use crate::native;
use explorer::fs::{self, Change};
use explorer::trash::{History, Item as TrashItem};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};
#[derive(Clone)]
pub enum Job {
    RestoreTrash {
        items: Vec<TrashItem>,
        destination: Option<PathBuf>,
    },
    DeleteTrash(Vec<TrashItem>),
    Transfer {
        paths: Vec<PathBuf>,
        destination: PathBuf,
        cut: bool,
    },
    Trash(Vec<PathBuf>),
    Create {
        parent: PathBuf,
        name: String,
        directory: bool,
    },
    Rename {
        path: PathBuf,
        name: String,
    },
    Compress(Vec<PathBuf>),
    Undo(Vec<Change>),
}
pub enum JobEvent {
    Progress {
        label: String,
        bytes: u64,
    },
    Done {
        changes: Vec<Change>,
        errors: Vec<String>,
        summary: String,
        remaining_undo: Option<Vec<Change>>,
        remaining_cut: Option<Vec<PathBuf>>,
    },
}
pub fn run(job: Job, cancel: Arc<AtomicBool>, tx: Sender<JobEvent>) {
    let mut changes = vec![];
    let mut errors = vec![];
    let mut bytes = 0;
    let mut remaining = None;
    let mut remaining_cut = None;
    let mut completed_cut = HashSet::new();
    let mut permanent_deletions = 0;
    let mut progress = |n| {
        bytes += n;
        let _ = tx.send(JobEvent::Progress {
            label: "Transferring files".into(),
            bytes,
        });
    };
    let result: std::io::Result<()> = match job {
        Job::RestoreTrash { items, destination } => {
            for item in items {
                if cancel.load(Ordering::Relaxed) {
                    errors.push("Restore cancelled".into());
                    break;
                }
                let target = destination
                    .as_ref()
                    .map(|directory| {
                        directory.join(
                            item.original
                                .as_ref()
                                .unwrap_or(&item.path)
                                .file_name()
                                .unwrap_or_default(),
                        )
                    })
                    .or(item.original.clone());
                let Some(target) = target else {
                    errors.push(format!(
                        "{}: original location is unknown; use Restore to…",
                        item.path.display()
                    ));
                    continue;
                };
                match item
                    .restore(&target, &cancel, &mut progress)
                    .and_then(|()| Change::moved(item.path.clone(), target))
                {
                    Ok(change) => changes.push(change),
                    Err(error) => errors.push(format!("{}: {error}", item.path.display())),
                }
            }
            Ok(())
        }
        Job::DeleteTrash(items) => {
            for item in items {
                if cancel.load(Ordering::Relaxed) {
                    errors.push("Permanent deletion cancelled".into());
                    break;
                }
                match item.delete_permanently() {
                    Ok(()) => permanent_deletions += 1,
                    Err(error) => errors.push(format!("{}: {error}", item.path.display())),
                }
            }
            Ok(())
        }
        Job::Transfer {
            paths,
            destination,
            cut,
        } => {
            if cut {
                remaining_cut = Some(paths.clone());
            }
            for path in paths {
                if cancel.load(Ordering::Relaxed) {
                    errors.push("Cancelled. Completed items were kept.".into());
                    break;
                }
                if cut
                    && std::fs::canonicalize(&destination).is_ok_and(|dest| {
                        path.parent()
                            .and_then(|p| std::fs::canonicalize(p).ok())
                            .as_ref()
                            == Some(&dest)
                    })
                {
                    continue;
                }
                let name = path.file_name().unwrap_or_default();
                // Every collision gets a distinct name. Existing items are never replaced or merged.
                let result = fs::unique_path(&destination, name, &cancel).and_then(|target| {
                    if cut {
                        fs::move_item(&path, &target, &cancel, &mut progress)?;
                        completed_cut.insert(path.clone());
                        Change::moved(path.clone(), target)
                    } else {
                        fs::copy(&path, &target, &cancel, &mut progress)?;
                        Change::created(target)
                    }
                });
                match result {
                    Ok(c) => changes.push(c),
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
            Ok(())
        }
        Job::Trash(paths) => {
            for path in paths {
                if cancel.load(Ordering::Relaxed) {
                    errors.push("Cancelled".into());
                    break;
                }
                match native::trash(&path).and_then(|to| {
                    if let Err(error) = History::remember_saved(&path, &to) {
                        errors.push(format!("Could not save restore location: {error}"));
                    }
                    Change::moved(path.clone(), to)
                }) {
                    Ok(c) => changes.push(c),
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
            Ok(())
        }
        Job::Create {
            parent,
            name,
            directory,
        } => fs::create(&parent, &name, directory)
            .and_then(|p| Change::created(p).map(|c| changes.push(c))),
        Job::Rename { path, name } => {
            fs::rename(&path, &name).and_then(|to| Change::moved(path, to).map(|c| changes.push(c)))
        }
        Job::Compress(paths) => {
            for path in paths {
                if cancel.load(Ordering::Relaxed) {
                    errors.push("Cancelled".into());
                    break;
                }
                let name = format!(
                    "{}.zip",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                let target = fs::unique_path(
                    path.parent().unwrap_or(std::path::Path::new("/")),
                    name.as_ref(),
                    &cancel,
                );
                match target.and_then(|target| {
                    native::compress(&path, &target).and_then(|_| Change::created(target))
                }) {
                    Ok(c) => changes.push(c),
                    Err(e) => errors.push(format!("{}: {e}", path.display())),
                }
            }
            Ok(())
        }
        Job::Undo(mut records) => {
            // Reverse one item at a time. Failed or cancelled records stay on the undo stack.
            let mut failed = vec![];
            while let Some(c) = records.pop() {
                if cancel.load(Ordering::Relaxed) {
                    records.push(c);
                    break;
                }
                if !c.identity.matches(&c.current) {
                    errors.push(format!(
                        "{} changed identity; undo skipped it.",
                        c.current.display()
                    ));
                    failed.push(c);
                    continue;
                }
                let r = if let Some(original) = &c.original {
                    fs::move_item(&c.current, original, &cancel, &mut progress).map(|()| {
                        if explorer::trash::roots().iter().any(|root| {
                            original
                                .parent()
                                .and_then(|p| std::fs::canonicalize(p).ok())
                                == std::fs::canonicalize(root).ok()
                        }) {
                            if let Err(error) = History::remember_saved(&c.current, original) {
                                errors.push(format!("Could not save restore location: {error}"));
                            }
                        }
                    })
                } else {
                    native::trash(&c.current).map(|to| {
                        if let Err(error) = History::remember_saved(&c.current, &to) {
                            errors.push(format!("Could not save restore location: {error}"));
                        }
                    })
                };
                if let Err(e) = r {
                    errors.push(format!("{}: {e}", c.current.display()));
                    failed.push(c);
                }
            }
            failed.reverse();
            records.extend(failed);
            remaining = Some(records);
            Ok(())
        }
    };
    if let Err(e) = result {
        errors.push(e.to_string());
    }
    let summary = if remaining.is_some() {
        if errors.is_empty() {
            "Undo complete".into()
        } else {
            "Some items could not be undone".into()
        }
    } else {
        format!(
            "{} item{} completed",
            changes.len() + permanent_deletions,
            if changes.len() + permanent_deletions == 1 {
                ""
            } else {
                "s"
            }
        )
    };
    if let Some(pending) = &mut remaining_cut {
        pending.retain(|path| !completed_cut.contains(path));
    }
    let _ = tx.send(JobEvent::Done {
        changes,
        errors,
        summary,
        remaining_undo: remaining,
        remaining_cut,
    });
}
