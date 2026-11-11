//! Trash capabilities, reversible restores, and persistent original locations.
use crate::{
    fs::{self as files, DirectoryAccess, Identity, Listing, PrivateStage},
    session,
};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub fn home_root() -> PathBuf {
    session::home().join(".Trash")
}
fn normalize(path: &Path) -> io::Result<PathBuf> {
    Ok(fs::canonicalize(
        path.parent()
            .ok_or_else(|| io::Error::other("Missing item parent"))?,
    )?
    .join(
        path.file_name()
            .ok_or_else(|| io::Error::other("Missing item name"))?,
    ))
}

/// Include the user's Trash on mounted volumes without traversing their files.
pub fn roots() -> Vec<PathBuf> {
    let mut roots = vec![home_root()];
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    let uid = unsafe { geteuid() };
    if let Ok(volumes) = fs::read_dir("/Volumes") {
        for volume in volumes.flatten() {
            let root = volume.path().join(format!(".Trashes/{uid}"));
            if root.is_dir()
                && !roots
                    .iter()
                    .any(|p| fs::canonicalize(p).ok() == fs::canonicalize(&root).ok())
            {
                roots.push(root);
            }
        }
    }
    roots
}

pub fn list(cancel: &AtomicBool) -> io::Result<Listing> {
    let mut listing = Listing::default();
    for root in roots() {
        match files::list(&root, true, cancel) {
            Ok(mut contents) => {
                contents
                    .entries
                    .retain(|e| e.name != ".DS_Store" && e.name != ".localized");
                listing.entries.extend(contents.entries);
                listing.skipped += contents.skipped;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if root == home_root() => return Err(error),
            Err(_) => listing.skipped += 1,
        }
    }
    Ok(listing)
}

#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    pub original: Option<PathBuf>,
    root: PathBuf,
    roots: Vec<PathBuf>,
    root_identity: Identity,
    identity: Identity,
}
struct Claimed {
    stage: PrivateStage,
    _access: Option<DirectoryAccess>,
}
impl Claimed {
    fn preserve(self) -> PathBuf {
        let recovery = self.stage.path.clone();
        let Self { stage, _access } = self;
        drop(_access);
        std::mem::forget(stage);
        recovery
    }
}
impl Item {
    /// Only top-level entries under an explicitly supplied Trash root qualify.
    pub fn read(path: &Path, allowed_roots: &[PathBuf], history: &History) -> io::Result<Self> {
        let parent = fs::canonicalize(
            path.parent()
                .ok_or_else(|| io::Error::other("Missing Trash folder"))?,
        )?;
        let roots: Vec<_> = allowed_roots
            .iter()
            .filter_map(|root| fs::canonicalize(root).ok())
            .collect();
        let root = roots
            .iter()
            .find(|root| **root == parent)
            .cloned()
            .ok_or_else(|| io::Error::other("This item is not inside an allowed Trash folder"))?;
        if root.parent().is_none() {
            return Err(io::Error::other(
                "The filesystem root is not a Trash folder",
            ));
        }
        let path = root.join(
            path.file_name()
                .ok_or_else(|| io::Error::other("Missing item name"))?,
        );
        Ok(Self {
            original: history.original(&path),
            root_identity: Identity::read(&root)?,
            identity: Identity::read(&path)?,
            root,
            roots,
            path,
        })
    }
    fn verify(&self) -> io::Result<()> {
        if !self.root_identity.matches(&self.root) || !self.identity.matches(&self.path) {
            return Err(io::Error::other(
                "The Trash item changed since confirmation. Refresh and try again.",
            ));
        }
        Ok(())
    }
    fn claim(&self) -> io::Result<Claimed> {
        self.verify()?;
        let stage = PrivateStage::new(&self.root, "trash")?;
        // macOS requires write access when changing a directory's parent. Use
        // an opened descriptor so restoring its mode cannot touch a replacement.
        let access = DirectoryAccess::acquire(&self.path, &self.identity)?;
        let claimed = Claimed {
            stage,
            _access: access,
        };
        let payload = claimed.stage.path.join("item");
        files::rename_exclusive(&self.path, &payload)?;
        if !self.identity.matches(&payload) {
            // Never let stage cleanup delete a replacement claimed during a race.
            let recovered = files::rename_exclusive(&payload, &self.path);
            if recovered.is_err() {
                let recovery = claimed.preserve();
                return Err(io::Error::other(format!(
                    "Trash item changed; preserved it in {}",
                    recovery.display()
                )));
            }
            return Err(io::Error::other(
                "Trash item changed; deletion or restore was cancelled",
            ));
        }
        Ok(claimed)
    }
    fn recover(&self, claimed: Claimed, error: io::Error) -> io::Error {
        if files::rename_exclusive(&claimed.stage.path.join("item"), &self.path).is_err() {
            let recovery = claimed.preserve();
            io::Error::new(
                error.kind(),
                format!(
                    "{error}; remaining item preserved in {}",
                    recovery.display()
                ),
            )
        } else {
            error
        }
    }
    pub fn restore(
        &self,
        destination: &Path,
        cancel: &AtomicBool,
        progress: impl FnMut(u64),
    ) -> io::Result<()> {
        let parent = fs::canonicalize(
            destination
                .parent()
                .ok_or_else(|| io::Error::other("Missing restore destination"))?,
        )?;
        if self.roots.iter().any(|root| parent.starts_with(root)) {
            return Err(io::Error::other("Choose a restore location outside Trash"));
        }
        let stage = self.claim()?;
        let payload = stage.stage.path.join("item");
        let result = (|| {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Restore cancelled",
                ));
            }
            match files::rename_exclusive(&payload, destination) {
                Ok(()) => Ok(()),
                Err(error) if error.raw_os_error() == Some(18) => {
                    if let Some(access) = &stage._access {
                        access.restore()?;
                    }
                    files::copy(&payload, destination, cancel, progress)?;
                    files::remove_private_tree(&payload)
                }
                Err(error) => Err(error),
            }
        })();
        if let Err(error) = result {
            return Err(self.recover(stage, error));
        }
        Ok(())
    }
    pub fn delete_permanently(&self) -> io::Result<()> {
        let stage = self.claim()?;
        if let Err(error) = files::remove_private_tree(&stage.stage.path.join("item")) {
            return Err(self.recover(stage, error));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Record {
    trashed: PathBuf,
    original: PathBuf,
    identity: Identity,
}
#[derive(Clone, Debug, Default)]
pub struct History {
    records: Vec<Record>,
}
impl History {
    pub fn location() -> PathBuf {
        session::home().join("Library/Application Support/Explorer/trash-history")
    }
    pub fn load(path: &Path) -> Self {
        let mut text = String::new();
        if fs::File::open(path)
            .and_then(|file| file.take(4 * 1024 * 1024).read_to_string(&mut text))
            .is_err()
        {
            return Self::default();
        }
        let mut records = vec![];
        for line in text.lines().take(4096) {
            let fields: Vec<_> = line.split(' ').collect();
            if fields.len() != 4 {
                continue;
            }
            let (Some(trashed), Some(original), Ok(dev), Ok(ino)) = (
                session::decode_path(fields[0]),
                session::decode_path(fields[1]),
                fields[2].parse(),
                fields[3].parse(),
            ) else {
                continue;
            };
            let identity = Identity { dev, ino };
            if trashed.is_absolute() && original.is_absolute() && identity.matches(&trashed) {
                records.push(Record {
                    trashed,
                    original,
                    identity,
                });
            }
        }
        Self { records }
    }
    pub fn original(&self, path: &Path) -> Option<PathBuf> {
        let path = normalize(path).ok()?;
        self.records
            .iter()
            .find(|r| r.trashed == path && r.identity.matches(&path))
            .map(|r| r.original.clone())
    }
    pub fn remember(&mut self, original: &Path, trashed: &Path) -> io::Result<()> {
        let trashed = normalize(trashed)?;
        let identity = Identity::read(&trashed)?;
        self.records
            .retain(|r| r.trashed != trashed && r.identity.matches(&r.trashed));
        if self.records.len() >= 4096 {
            self.records.remove(0);
        }
        self.records.push(Record {
            trashed,
            original: original.to_path_buf(),
            identity,
        });
        Ok(())
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing Trash history folder"))?;
        fs::create_dir_all(parent)?;
        let stage = PrivateStage::new(parent, "history")?;
        let temporary = stage.path.join("history");
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        for record in &self.records {
            writeln!(
                file,
                "{} {} {} {}",
                session::encode_path(&record.trashed),
                session::encode_path(&record.original),
                record.identity.dev,
                record.identity.ino
            )?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, path)
    }
    pub fn remember_saved(original: &Path, trashed: &Path) -> io::Result<()> {
        let path = Self::location();
        let mut history = Self::load(&path);
        history.remember(original, trashed)?;
        history.save(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        sync::atomic::AtomicU64,
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        directory: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "explorer-trash-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let root = directory.join("Trash");
            fs::create_dir_all(&root).unwrap();
            Self { directory, root }
        }
        fn item(&self, name: &str, history: &History) -> Item {
            Item::read(
                &self.root.join(name),
                std::slice::from_ref(&self.root),
                history,
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = files::remove_private_tree(&self.directory);
        }
    }
    #[test]
    fn original_locations_survive_restart_and_restore_without_overwriting() {
        let f = Fixture::new();
        let original = f.directory.join("original 你好.txt");
        let trashed = f.root.join("renamed.txt");
        fs::write(&original, "contents").unwrap();
        fs::rename(&original, &trashed).unwrap();
        let mut history = History::default();
        history.remember(&original, &trashed).unwrap();
        let index = f.directory.join("index");
        history.save(&index).unwrap();
        let history = History::load(&index);
        let item = f.item("renamed.txt", &history);
        assert_eq!(item.original, Some(original.clone()));
        item.restore(&original, &AtomicBool::new(false), |_| {})
            .unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), "contents");
        assert!(!trashed.exists());
        assert!(History::load(&index).records.is_empty());
    }
    #[test]
    fn restore_collision_and_cancellation_keep_the_item_in_trash() {
        let f = Fixture::new();
        fs::write(f.root.join("item"), "trash contents").unwrap();
        let item = f.item("item", &History::default());
        let target = f.directory.join("existing");
        fs::write(&target, "existing data").unwrap();
        assert!(
            item.restore(&target, &AtomicBool::new(false), |_| {})
                .is_err()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "existing data");
        assert_eq!(fs::read_to_string(&item.path).unwrap(), "trash contents");
        let target = f.directory.join("cancelled");
        assert!(
            item.restore(&target, &AtomicBool::new(true), |_| {})
                .is_err()
        );
        assert!(!target.exists());
        assert!(item.path.exists());
        assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
    }
    #[test]
    fn unknown_original_can_be_restored_to_a_chosen_location() {
        let f = Fixture::new();
        fs::write(f.root.join("item"), "contents").unwrap();
        let item = f.item("item", &History::default());
        assert!(item.original.is_none());
        let target = f.directory.join("recovered");
        item.restore(&target, &AtomicBool::new(false), |_| {})
            .unwrap();
        assert_eq!(fs::read_to_string(target).unwrap(), "contents");
    }
    #[test]
    fn restoring_a_readonly_folder_preserves_its_permissions() {
        let f = Fixture::new();
        let folder = f.root.join("folder");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("file"), "contents").unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let target = f.directory.join("restored");
        f.item("folder", &History::default())
            .restore(&target, &AtomicBool::new(false), |_| {})
            .unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_eq!(fs::read_to_string(target.join("file")).unwrap(), "contents");
    }
    #[test]
    fn permanent_deletion_rejects_outside_paths_and_changed_items() {
        let f = Fixture::new();
        let outside = f.directory.join("keep");
        fs::write(&outside, "outside").unwrap();
        assert!(Item::read(&outside, std::slice::from_ref(&f.root), &History::default()).is_err());
        fs::write(f.root.join("item"), "old").unwrap();
        let other_trash = f.directory.join("Other Trash");
        fs::create_dir(&other_trash).unwrap();
        let item = Item::read(
            &f.root.join("item"),
            &[f.root.clone(), other_trash.clone()],
            &History::default(),
        )
        .unwrap();
        assert!(
            item.restore(&other_trash.join("item"), &AtomicBool::new(false), |_| {})
                .is_err()
        );
        assert!(item.path.exists());
        fs::rename(&item.path, f.root.join("old")).unwrap();
        fs::write(&item.path, "replacement").unwrap();
        assert!(item.delete_permanently().is_err());
        assert_eq!(fs::read_to_string(&item.path).unwrap(), "replacement");
        assert_eq!(fs::read_to_string(outside).unwrap(), "outside");
    }
    #[test]
    fn permanently_deleting_a_readonly_folder_never_follows_internal_links() {
        let f = Fixture::new();
        let outside = f.directory.join("keep");
        fs::write(&outside, "outside").unwrap();
        let folder = f.root.join("folder");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("inside"), "delete").unwrap();
        symlink(&outside, folder.join("link")).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        f.item("folder", &History::default())
            .delete_permanently()
            .unwrap();
        assert!(!folder.exists());
        assert_eq!(fs::read_to_string(outside).unwrap(), "outside");
        assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
    }
    #[test]
    fn original_location_is_not_reused_for_a_replacement() {
        let f = Fixture::new();
        let trashed = f.root.join("item");
        fs::write(&trashed, "old").unwrap();
        let mut history = History::default();
        history
            .remember(&f.directory.join("original"), &trashed)
            .unwrap();
        fs::rename(&trashed, f.root.join("old")).unwrap();
        fs::write(&trashed, "new").unwrap();
        assert!(history.original(&trashed).is_none());
        let index = f.directory.join("index");
        history.save(&index).unwrap();
        assert!(History::load(&index).original(&trashed).is_none());
    }
}
