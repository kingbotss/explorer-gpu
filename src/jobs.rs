use crate::native;
use explorer::fs::{self, Change};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};
#[derive(Clone)]
pub enum Job {
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
    },
}
pub fn run(job: Job, cancel: Arc<AtomicBool>, tx: Sender<JobEvent>) {
    let mut changes = vec![];
    let mut errors = vec![];
    let mut bytes = 0;
    let mut remaining = None;
    let mut progress = |n| {
        bytes += n;
        let _ = tx.send(JobEvent::Progress {
            label: "Transferring files".into(),
            bytes,
        });
    };
    let result: std::io::Result<()> = match job {
        Job::Transfer {
            paths,
            destination,
            cut,
        } => {
            for path in paths {
                if cancel.load(Ordering::Relaxed) {
                    errors.push(
                        "Cancelled. Completed items were kept; unfinished copies were removed."
                            .into(),
                    );
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
                let target = fs::unique_path(&destination, name);
                let r = if cut {
                    fs::move_item(&path, &target, &cancel, &mut progress)
                } else {
                    fs::copy(&path, &target, &cancel, &mut progress)
                };
                match r.and_then(|_| {
                    if cut {
                        Change::moved(path.clone(), target)
                    } else {
                        Change::created(target)
                    }
                }) {
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
                match native::trash(&path).and_then(|to| Change::moved(path.clone(), to)) {
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
                );
                match native::compress(&path, &target).and_then(|_| Change::created(target)) {
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
                    fs::move_item(&c.current, original, &cancel, &mut progress)
                } else {
                    native::trash(&c.current).map(|_| ())
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
            changes.len(),
            if changes.len() == 1 { "" } else { "s" }
        )
    };
    let _ = tx.send(JobEvent::Done {
        changes,
        errors,
        summary,
        remaining_undo: remaining,
    });
}
