//! Worker regressions using isolated files; native Trash/ZIP APIs must not run.
#![allow(dead_code)]
#[path = "../src/jobs.rs"]
mod jobs;
mod native {
    pub fn trash(_: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
        panic!("This test must not access the user's Trash")
    }
    pub fn compress(_: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        panic!("This test does not use the native ZIP integration")
    }
}
use explorer::{
    fs::{Change, Entry},
    trash::{History, Item},
};
use jobs::{Job, JobEvent};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "explorer-job-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Done {
    changes: Vec<Change>,
    errors: Vec<String>,
    summary: String,
    remaining_cut: Option<Vec<PathBuf>>,
}
fn run(job: Job, cancelled: bool) -> Done {
    let (tx, rx) = mpsc::channel();
    jobs::run(job, Arc::new(AtomicBool::new(cancelled)), tx);
    for event in rx {
        if let JobEvent::Done {
            changes,
            errors,
            summary,
            remaining_cut,
            ..
        } = event
        {
            return Done {
                changes,
                errors,
                summary,
                remaining_cut,
            };
        }
    }
    panic!("Missing completion event")
}
#[test]
fn partial_cut_keeps_only_unmoved_sources_for_retry() {
    let f = Fixture::new();
    fs::create_dir(f.path("destination")).unwrap();
    fs::write(f.path("good"), "contents").unwrap();
    let result = run(
        Job::Transfer {
            paths: vec![f.path("good"), f.path("missing")],
            destination: f.path("destination"),
            cut: true,
        },
        false,
    );
    assert_eq!(result.changes.len(), 1);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.remaining_cut, Some(vec![f.path("missing")]));
    assert_eq!(
        fs::read_to_string(f.path("destination/good")).unwrap(),
        "contents"
    );
    assert!(!f.path("good").exists());
}
#[test]
fn cancelling_a_cut_keeps_all_pending_sources_and_same_folder_paste_is_retryable() {
    let f = Fixture::new();
    let source = f.path("source");
    fs::write(&source, "keep").unwrap();
    let result = run(
        Job::Transfer {
            paths: vec![source.clone()],
            destination: f.0.clone(),
            cut: true,
        },
        true,
    );
    assert_eq!(result.remaining_cut, Some(vec![source.clone()]));
    assert!(result.changes.is_empty());
    let result = run(
        Job::Transfer {
            paths: vec![source.clone()],
            destination: f.0.clone(),
            cut: true,
        },
        false,
    );
    assert_eq!(result.remaining_cut, Some(vec![source.clone()]));
    assert_eq!(fs::read_to_string(source).unwrap(), "keep");
}
#[test]
fn successful_cut_clears_pending_sources_but_copy_does_not_change_cut_state() {
    let f = Fixture::new();
    fs::create_dir(f.path("destination")).unwrap();
    fs::write(f.path("source"), "keep").unwrap();
    let copy = run(
        Job::Transfer {
            paths: vec![f.path("source")],
            destination: f.path("destination"),
            cut: false,
        },
        false,
    );
    assert!(copy.remaining_cut.is_none());
    let cut = run(
        Job::Transfer {
            paths: vec![f.path("source")],
            destination: f.path("destination"),
            cut: true,
        },
        false,
    );
    assert_eq!(cut.remaining_cut, Some(vec![]));
    assert!(cut.errors.is_empty());
}
#[test]
fn deleting_a_confirmed_trash_snapshot_preserves_later_arrivals() {
    let f = Fixture::new();
    let root = f.path("Trash");
    fs::create_dir(&root).unwrap();
    let mut items = vec![];
    for name in ["one", "two"] {
        let path = root.join(name);
        fs::write(&path, "delete").unwrap();
        items.push(Item::read(&path, std::slice::from_ref(&root), &History::default()).unwrap());
    }
    fs::write(root.join("later"), "keep").unwrap();
    let result = run(Job::DeleteTrash(items), false);
    assert!(result.errors.is_empty());
    assert!(
        result.changes.is_empty(),
        "Permanent deletion must not create undo records"
    );
    assert_eq!(result.summary, "2 items completed");
    assert_eq!(fs::read_to_string(root.join("later")).unwrap(), "keep");
    assert!(!root.join("one").exists());
    assert!(!root.join("two").exists());
}
#[test]
fn restore_to_selected_folder_uses_original_name_and_is_undoable() {
    let f = Fixture::new();
    let root = f.path("Trash");
    fs::create_dir(&root).unwrap();
    fs::create_dir(f.path("restore-here")).unwrap();
    let path = root.join("renamed-in-trash");
    fs::write(&path, "restore").unwrap();
    let mut history = History::default();
    history.remember(&f.path("original-name"), &path).unwrap();
    let item = Item::read(&path, std::slice::from_ref(&root), &history).unwrap();
    let result = run(
        Job::RestoreTrash {
            items: vec![item],
            destination: Some(f.path("restore-here")),
        },
        false,
    );
    assert!(result.errors.is_empty());
    assert_eq!(result.changes.len(), 1);
    assert_eq!(
        fs::read_to_string(f.path("restore-here/original-name")).unwrap(),
        "restore"
    );
    assert!(!path.exists());
    let _ = Entry::read(f.path("restore-here/original-name")).unwrap();
}
