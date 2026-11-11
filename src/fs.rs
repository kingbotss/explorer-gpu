#[cfg(unix)]
use std::os::unix::{
    ffi::OsStrExt,
    fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
};
use std::{
    cmp::Ordering,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub symlink: bool,
    pub application: bool,
    pub bytes: u64,
    pub modified: SystemTime,
    pub created: Option<SystemTime>,
    pub readonly: bool,
    pub mode: u32,
    pub kind: String,
}
impl Entry {
    pub fn read(path: PathBuf) -> io::Result<Self> {
        let link = fs::symlink_metadata(&path)?;
        let metadata = if link.is_symlink() {
            fs::metadata(&path).unwrap_or_else(|_| link.clone())
        } else {
            link.clone()
        };
        let directory = metadata.is_dir();
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let application = directory
            && if link.is_symlink() {
                fs::canonicalize(&path)
                    .ok()
                    .and_then(|p| p.extension().map(|e| e.eq_ignore_ascii_case("app")))
                    .unwrap_or(false)
            } else {
                ext == "app"
            };
        let kind = if link.is_symlink() {
            "Symbolic link".into()
        } else if application {
            "Application".into()
        } else if directory {
            "Folder".into()
        } else {
            match ext.as_str() {
                "jpg" | "jpeg" | "png" | "heic" | "gif" | "webp" | "tiff" | "svg" => "Image".into(),
                "mp4" | "mov" | "mkv" | "webm" => "Video".into(),
                "mp3" | "wav" | "m4a" | "flac" | "aiff" => "Audio".into(),
                "zip" | "tar" | "gz" | "7z" | "rar" => "Archive".into(),
                "pdf" => "PDF document".into(),
                "rs" | "js" | "ts" | "tsx" | "py" | "swift" | "go" | "html" | "css" | "json"
                | "toml" | "yaml" => "Source code".into(),
                "txt" | "md" | "log" | "csv" => "Text document".into(),
                "" => "File".into(),
                _ => format!("{} file", ext.to_uppercase()),
            }
        };
        Ok(Self {
            name: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            path,
            directory,
            symlink: link.is_symlink(),
            application,
            bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            created: metadata.created().ok(),
            readonly: metadata.permissions().readonly(),
            mode: metadata.permissions().mode(),
            kind,
        })
    }
    pub fn navigable(&self) -> bool {
        self.directory && !self.application
    }
    /// Listing keeps broken links visible; opening requires a reachable target.
    pub fn read_for_open(path: PathBuf) -> io::Result<Self> {
        fs::metadata(&path)?;
        Self::read(path)
    }
    pub fn icon(&self) -> &'static str {
        if self.symlink {
            "link"
        } else if self.directory {
            "folder"
        } else {
            match self.kind.as_str() {
                "Image" => "image",
                "Video" => "video",
                "Audio" => "music",
                "Archive" => "archive",
                "Source code" => "code",
                _ => "file",
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Modified,
    Kind,
    Size,
}
pub fn sort(entries: &mut [Entry], key: Sort, descending: bool) {
    entries.sort_by(|a, b| {
        let grouping = b.directory.cmp(&a.directory);
        if grouping != Ordering::Equal {
            return grouping;
        }
        let order = match key {
            Sort::Name => natural_cmp(&a.name.to_lowercase(), &b.name.to_lowercase()),
            Sort::Modified => a.modified.cmp(&b.modified),
            Sort::Kind => a.kind.cmp(&b.kind),
            Sort::Size => a.bytes.cmp(&b.bytes),
        }
        .then_with(|| natural_cmp(&a.name.to_lowercase(), &b.name.to_lowercase()))
        .then_with(|| a.path.cmp(&b.path));
        if descending { order.reverse() } else { order }
    });
}
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek(), b.peek()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let x: String = std::iter::from_fn(|| {
                    a.peek()
                        .copied()
                        .filter(char::is_ascii_digit)
                        .map(|_| a.next().unwrap())
                })
                .collect();
                let y: String = std::iter::from_fn(|| {
                    b.peek()
                        .copied()
                        .filter(char::is_ascii_digit)
                        .map(|_| b.next().unwrap())
                })
                .collect();
                let (nx, ny) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let c = nx
                    .len()
                    .cmp(&ny.len())
                    .then_with(|| nx.cmp(ny))
                    .then_with(|| x.len().cmp(&y.len()));
                if c != Ordering::Equal {
                    return c;
                }
            }
            (Some(_), Some(_)) => {
                let c = a.next().cmp(&b.next());
                if c != Ordering::Equal {
                    return c;
                }
            }
            _ => return a.next().cmp(&b.next()),
        }
    }
}
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub skipped: usize,
    pub truncated: bool,
}
#[derive(Clone, Debug, Default)]
pub struct FolderSize {
    pub bytes: u64,
    pub files: u64,
    pub folders: u64,
    pub links: u64,
    pub skipped: u64,
}
/// Logical content size. Resolve a selected root link, but never traverse links
/// inside it. Include hidden files and count link lengths rather than targets.
pub fn folder_size(root: &Path, cancel: &AtomicBool) -> io::Result<FolderSize> {
    let root = fs::canonicalize(root)?;
    if !fs::metadata(&root)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "Expected a folder",
        ));
    }
    let mut size = FolderSize::default();
    let mut stack = vec![root.clone()];
    while let Some(path) = stack.pop() {
        if cancel.load(AtomicOrdering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Size calculation cancelled",
            ));
        }
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if path == root => {
                return Err(error);
            }
            Err(_) => {
                size.skipped += 1;
                continue;
            }
        };
        for entry in entries {
            if cancel.load(AtomicOrdering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Size calculation cancelled",
                ));
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    size.skipped += 1;
                    continue;
                }
            };
            match fs::symlink_metadata(entry.path()) {
                Ok(metadata) if metadata.is_symlink() => {
                    size.links += 1;
                    size.bytes = size.bytes.saturating_add(metadata.len());
                }
                Ok(metadata) if metadata.is_dir() => {
                    size.folders += 1;
                    stack.push(entry.path());
                }
                Ok(metadata) if metadata.is_file() => {
                    size.files += 1;
                    size.bytes = size.bytes.saturating_add(metadata.len());
                }
                Ok(_) => {}
                Err(_) => size.skipped += 1,
            }
        }
    }
    Ok(size)
}
pub fn list(path: &Path, hidden: bool, cancel: &AtomicBool) -> io::Result<Listing> {
    let mut result = Listing::default();
    for item in fs::read_dir(path)? {
        if cancel.load(AtomicOrdering::Relaxed) {
            break;
        }
        match item {
            Ok(item) => {
                if !hidden && item.file_name().as_bytes().first() == Some(&b'.') {
                    continue;
                }
                match Entry::read(item.path()) {
                    Ok(entry) => result.entries.push(entry),
                    Err(_) => result.skipped += 1,
                }
            }
            Err(_) => result.skipped += 1,
        }
    }
    Ok(result)
}
pub fn search(root: &Path, query: &str, hidden: bool, cancel: &AtomicBool) -> io::Result<Listing> {
    // Do not traverse directory symlinks: this prevents loops and escaping the search root.
    let mut stack = vec![root.to_path_buf()];
    let mut result = Listing::default();
    let query = query.to_lowercase();
    fs::read_dir(root)?;
    while let Some(path) = stack.pop() {
        if cancel.load(AtomicOrdering::Relaxed) {
            break;
        }
        match list(&path, hidden, cancel) {
            Ok(listing) => {
                result.skipped += listing.skipped;
                for entry in listing.entries {
                    if cancel.load(AtomicOrdering::Relaxed) {
                        break;
                    }
                    if entry.navigable() && !entry.symlink {
                        stack.push(entry.path.clone());
                    }
                    if entry.name.to_lowercase().contains(&query) {
                        result.entries.push(entry);
                        if result.entries.len() >= 10_000 {
                            result.truncated = true;
                            return Ok(result);
                        }
                    }
                }
            }
            Err(_) => result.skipped += 1,
        }
    }
    Ok(result)
}
pub fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\0')
        || name.contains(':')
    {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter a name without /, : or null characters.",
        ))
    } else {
        Ok(())
    }
}
pub fn create(parent: &Path, name: &str, directory: bool) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = parent.join(name);
    if directory {
        fs::create_dir(&path)?;
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
    }
    Ok(path)
}
pub fn unique_path(
    parent: &Path,
    name: &std::ffi::OsStr,
    cancel: &AtomicBool,
) -> io::Result<PathBuf> {
    cancelled(cancel)?;
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let p = Path::new(name);
    for n in 1.. {
        cancelled(cancel)?;
        let mut candidate = p.file_stem().unwrap_or(name).to_os_string();
        candidate.push(if n == 1 {
            " copy".into()
        } else {
            format!(" copy {n}")
        });
        if let Some(extension) = p.extension() {
            candidate.push(".");
            candidate.push(extension);
        }
        let candidate = parent.join(candidate);
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }
    unreachable!()
}

#[cfg(target_os = "macos")]
pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // RENAME_EXCL is atomic even when another process creates the destination.
    if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 4) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[cfg(not(target_os = "macos"))]
pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ));
    }
    fs::rename(from, to)
}
pub fn rename(path: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    let target = path
        .parent()
        .ok_or_else(|| io::Error::other("Cannot rename the filesystem root"))?
        .join(name);
    if path != target {
        rename_exclusive(path, &target)?;
    }
    Ok(target)
}
fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(AtomicOrdering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Operation cancelled",
        ))
    } else {
        Ok(())
    }
}
fn validate_transfer(from: &Path, to: &Path) -> io::Result<()> {
    let source = fs::canonicalize(from).or_else(|e| {
        if fs::symlink_metadata(from)?.is_symlink() {
            Ok(from.to_path_buf())
        } else {
            Err(e)
        }
    })?;
    let target = fs::canonicalize(
        to.parent()
            .ok_or_else(|| io::Error::other("Missing destination parent"))?,
    )?
    .join(
        to.file_name()
            .ok_or_else(|| io::Error::other("Missing destination name"))?,
    );
    if source == target || (fs::symlink_metadata(from)?.is_dir() && target.starts_with(&source)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A folder cannot be copied or moved into itself.",
        ));
    }
    Ok(())
}
// This is used only inside a private staging directory, never on a public
// destination or a source. Restore owner access before removing read-only trees.
pub(crate) fn remove_private_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
        )?;
        for child in fs::read_dir(path)? {
            remove_private_tree(&child?.path())?;
        }
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}
pub(crate) struct PrivateStage {
    pub(crate) path: PathBuf,
    identity: Identity,
}
impl PrivateStage {
    pub(crate) fn new(parent: &Path, kind: &str) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = parent.join(format!(
                ".explorer-{kind}-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {
                    return Ok(Self {
                        identity: Identity::read(&path)?,
                        path,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
    pub(crate) fn cleanup(&self) -> io::Result<()> {
        if fs::symlink_metadata(&self.path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
            return Ok(());
        }
        if !self.identity.matches(&self.path) {
            return Err(io::Error::other(format!(
                "Temporary folder changed identity; preserved {}",
                self.path.display()
            )));
        }
        remove_private_tree(&self.path)
    }
}
impl Drop for PrivateStage {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
pub(crate) struct DirectoryAccess {
    file: File,
    permissions: fs::Permissions,
}
impl DirectoryAccess {
    pub(crate) fn acquire(path: &Path, expected: &Identity) -> io::Result<Option<Self>> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o200 != 0 {
            return Ok(None);
        }
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if (Identity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }) != *expected
        {
            return Err(io::Error::other("Folder changed before moving"));
        }
        let permissions = metadata.permissions();
        file.set_permissions(fs::Permissions::from_mode(permissions.mode() | 0o200))?;
        Ok(Some(Self { file, permissions }))
    }
    pub(crate) fn restore(&self) -> io::Result<()> {
        self.file.set_permissions(self.permissions.clone())
    }
}
impl Drop for DirectoryAccess {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
fn copy_inner(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> io::Result<()> {
    cancelled(cancel)?;
    let meta = fs::symlink_metadata(from)?;
    if meta.is_symlink() {
        symlink(fs::read_link(from)?, to)?;
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir(to)?;
        (|| {
            for item in fs::read_dir(from)? {
                let item = item?;
                copy_inner(&item.path(), &to.join(item.file_name()), cancel, progress)?;
            }
            fs::set_permissions(to, meta.permissions())?;
            Ok(())
        })()
    } else if meta.is_file() {
        let mut source = File::open(from)?;
        let mut dest = OpenOptions::new().write(true).create_new(true).open(to)?;
        let result = (|| {
            let mut buf = vec![0u8; 1024 * 1024];
            loop {
                cancelled(cancel)?;
                let n = source.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                dest.write_all(&buf[..n])?;
                progress(n as u64);
            }
            dest.sync_all()?;
            fs::set_permissions(to, meta.permissions())?;
            // Retain timestamps without adding an external dependency.
            let times = fs::FileTimes::new().set_modified(meta.modified()?);
            dest.set_times(times)?;
            Ok(())
        })();
        result
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Sockets, devices and named pipes are not copied.",
        ))
    }
}
pub fn copy(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> io::Result<()> {
    validate_transfer(from, to)?;
    cancelled(cancel)?;
    match fs::symlink_metadata(to) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination already exists",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let stage = PrivateStage::new(
        to.parent()
            .ok_or_else(|| io::Error::other("Missing destination parent"))?,
        "copy",
    )?;
    let payload = stage.path.join("item");
    let result = copy_inner(from, &payload, cancel, &mut progress)
        .and_then(|()| cancelled(cancel))
        .and_then(|()| {
            let _access = DirectoryAccess::acquire(&payload, &Identity::read(&payload)?)?;
            rename_exclusive(&payload, to)
        });
    if let Err(error) = result {
        if let Err(cleanup) = stage.cleanup() {
            return Err(io::Error::new(
                error.kind(),
                format!("{error}; cleanup failed: {cleanup}"),
            ));
        }
        return Err(error);
    }
    Ok(())
}
pub fn move_item(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> io::Result<()> {
    validate_transfer(from, to)?;
    cancelled(cancel)?;
    match rename_exclusive(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => {
            copy(from, to, cancel, progress)?;
            // A completed cross-volume copy is preserved if deleting the source fails.
            if fs::symlink_metadata(from)?.is_dir() {
                fs::remove_dir_all(from)
            } else {
                fs::remove_file(from)
            }
        }
        Err(e) => Err(e),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
}
impl Identity {
    pub fn read(path: &Path) -> io::Result<Self> {
        let m = fs::symlink_metadata(path)?;
        Ok(Self {
            dev: m.dev(),
            ino: m.ino(),
        })
    }
    pub fn matches(&self, path: &Path) -> bool {
        Self::read(path).is_ok_and(|id| id == *self)
    }
}
#[derive(Clone, Debug)]
pub struct Change {
    pub original: Option<PathBuf>,
    pub current: PathBuf,
    pub identity: Identity,
}
impl Change {
    pub fn created(path: PathBuf) -> io::Result<Self> {
        Ok(Self {
            identity: Identity::read(&path)?,
            original: None,
            current: path,
        })
    }
    pub fn moved(from: PathBuf, to: PathBuf) -> io::Result<Self> {
        Ok(Self {
            identity: Identity::read(&to)?,
            original: Some(from),
            current: to,
        })
    }
}

pub fn preview_text(path: &Path) -> io::Result<String> {
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::other(
            "Select a regular file for a text preview.",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(32 * 1024).read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Err(io::Error::other("Binary file · press Space for Quick Look"));
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if fs::metadata(path)?.len() > bytes.len() as u64 {
        text.push_str("\n\n… Preview limited to 32 KB. Open the file to read more.");
    }
    Ok(text)
}
pub fn human_size(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    for unit in ["KB", "MB", "GB", "TB", "PB"] {
        value /= 1000.;
        if value < 1000. || unit == "PB" {
            return format!("{value:.1} {unit}");
        }
    }
    unreachable!()
}
pub fn date(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs / 86400;
    // Gregorian date from Unix days (UTC), valid for the full UI date range.
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs % 86400 / 3600,
        secs % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "explorer-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn path(&self, p: &str) -> PathBuf {
            self.0.join(p)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = remove_private_tree(&self.0);
        }
    }
    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }
    fn assert_no_staging(parent: &Path) {
        assert!(!fs::read_dir(parent).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".explorer-copy-")
        }));
    }
    #[test]
    fn cancellation_preserves_a_destination_created_by_another_writer() {
        let f = Fixture::new();
        fs::write(f.path("source"), vec![0; 2 * 1024 * 1024]).unwrap();
        let cancel = no();
        let result = copy(&f.path("source"), &f.path("destination"), &cancel, |_| {
            fs::write(f.path("destination"), "independent data").unwrap();
            cancel.store(true, AtomicOrdering::Relaxed);
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(
            fs::read_to_string(f.path("destination")).unwrap(),
            "independent data"
        );
        assert_no_staging(&f.0);
    }
    #[test]
    fn a_readonly_root_folder_can_be_published_without_changing_its_mode() {
        let f = Fixture::new();
        let source = create(&f.0, "source", true).unwrap();
        fs::write(source.join("file"), "contents").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o555)).unwrap();
        copy(&source, &f.path("destination"), &no(), |_| {}).unwrap();
        assert_eq!(
            fs::metadata(f.path("destination"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o555
        );
        assert_eq!(
            fs::metadata(&source).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_eq!(
            fs::read_to_string(f.path("destination/file")).unwrap(),
            "contents"
        );
        assert_no_staging(&f.0);
    }
    #[test]
    fn copy_publish_preserves_a_destination_created_during_transfer() {
        let f = Fixture::new();
        fs::write(f.path("source"), "copy data").unwrap();
        let result = copy(&f.path("source"), &f.path("destination"), &no(), |_| {
            fs::write(f.path("destination"), "independent data").unwrap();
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(f.path("destination")).unwrap(),
            "independent data"
        );
        assert_no_staging(&f.0);
    }
    #[test]
    fn cancelling_after_a_readonly_subtree_cleans_the_entire_partial_copy() {
        let f = Fixture::new();
        let source = create(&f.0, "source", true).unwrap();
        fs::create_dir(source.join("one")).unwrap();
        fs::create_dir(source.join("two")).unwrap();
        let mut children = fs::read_dir(&source)
            .unwrap()
            .map(|entry| entry.unwrap().path());
        let readonly = children.next().unwrap();
        let later = children.next().unwrap();
        fs::write(readonly.join("file"), [1]).unwrap();
        fs::write(later.join("file"), vec![0; 2 * 1024 * 1024]).unwrap();
        fs::set_permissions(&readonly, fs::Permissions::from_mode(0o555)).unwrap();
        let cancel = no();
        let mut total = 0;
        let result = copy(&source, &f.path("destination"), &cancel, |bytes| {
            total += bytes;
            if total > 1 {
                cancel.store(true, AtomicOrdering::Relaxed);
            }
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!f.path("destination").exists());
        assert_eq!(
            fs::metadata(&readonly).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_no_staging(&f.0);
    }
    #[test]
    fn unique_path_returns_permission_errors_and_honors_cancellation() {
        let f = Fixture::new();
        let locked = create(&f.0, "locked", true).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0)).unwrap();
        let result = unique_path(&locked, "file".as_ref(), &no());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            unique_path(&f.0, "file".as_ref(), &AtomicBool::new(true))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
    }
    #[test]
    fn folder_size_includes_nested_and_hidden_files() {
        let f = Fixture::new();
        fs::create_dir_all(f.path("nested/empty")).unwrap();
        fs::write(f.path("a"), [0; 123]).unwrap();
        fs::write(f.path("nested/.hidden"), [0; 456]).unwrap();
        let size = folder_size(&f.0, &no()).unwrap();
        assert_eq!(
            (
                size.bytes,
                size.files,
                size.folders,
                size.links,
                size.skipped
            ),
            (579, 2, 2, 0, 0)
        );
    }
    #[test]
    fn folder_size_never_traverses_internal_symlinks_and_can_size_a_root_link() {
        let f = Fixture::new();
        fs::create_dir(f.path("root")).unwrap();
        fs::create_dir(f.path("outside")).unwrap();
        fs::write(f.path("outside/large"), [0; 5000]).unwrap();
        fs::write(f.path("root/file"), [0; 10]).unwrap();
        symlink(".", f.path("root/loop")).unwrap();
        symlink("../outside", f.path("root/link")).unwrap();
        symlink("root", f.path("root link")).unwrap();
        let size = folder_size(&f.path("root link"), &no()).unwrap();
        assert_eq!(
            (size.bytes, size.files, size.folders, size.links),
            (21, 1, 0, 2)
        );
    }
    #[test]
    fn folder_size_cancellation_and_invalid_roots_are_reported() {
        let f = Fixture::new();
        assert_eq!(
            folder_size(&f.0, &AtomicBool::new(true))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        fs::write(f.path("file"), "content").unwrap();
        assert_eq!(
            folder_size(&f.path("file"), &no()).unwrap_err().kind(),
            io::ErrorKind::NotADirectory
        );
        assert_eq!(
            folder_size(&f.path("missing"), &no()).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(folder_size(&f.0, &no()).unwrap().bytes, 7);
    }
    #[test]
    fn folder_size_marks_unreadable_subdirectories_as_partial() {
        let f = Fixture::new();
        fs::create_dir(f.path("locked")).unwrap();
        fs::write(f.path("locked/file"), [0; 500]).unwrap();
        fs::write(f.path("visible"), [0; 10]).unwrap();
        fs::set_permissions(f.path("locked"), fs::Permissions::from_mode(0)).unwrap();
        let size = folder_size(&f.0, &no());
        fs::set_permissions(f.path("locked"), fs::Permissions::from_mode(0o700)).unwrap();
        let size = size.unwrap();
        assert_eq!((size.bytes, size.skipped), (10, 1));
    }
    #[test]
    fn create_never_truncates_existing_file() {
        let f = Fixture::new();
        fs::write(f.path("a"), "keep").unwrap();
        assert!(create(&f.0, "a", false).is_err());
        assert_eq!(fs::read_to_string(f.path("a")).unwrap(), "keep");
    }
    #[test]
    fn names_reject_traversal_and_root() {
        for s in ["", ".", "..", "a/b", "a\0b", "a:b"] {
            assert!(validate_name(s).is_err());
        }
        assert!(validate_name("photo 你好.txt").is_ok());
    }
    #[test]
    fn listing_filters_hidden() {
        let f = Fixture::new();
        create(&f.0, ".hidden", false).unwrap();
        create(&f.0, "visible", false).unwrap();
        assert_eq!(list(&f.0, false, &no()).unwrap().entries.len(), 1);
        assert_eq!(list(&f.0, true, &no()).unwrap().entries.len(), 2);
    }
    #[test]
    fn sort_naturally_keeps_folders_first() {
        let f = Fixture::new();
        for n in ["file10", "file2", "file1"] {
            create(&f.0, n, false).unwrap();
        }
        create(&f.0, "z-folder", true).unwrap();
        let mut e = list(&f.0, false, &no()).unwrap().entries;
        sort(&mut e, Sort::Name, false);
        assert_eq!(
            e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["z-folder", "file1", "file2", "file10"]
        );
        sort(&mut e, Sort::Name, true);
        assert_eq!(e[0].name, "z-folder");
        assert_eq!(e[1].name, "file10");
    }
    #[test]
    fn copy_tree_and_progress() {
        let f = Fixture::new();
        let a = create(&f.0, "a", true).unwrap();
        fs::write(a.join("file"), "contents").unwrap();
        let mut bytes = 0;
        copy(&a, &f.path("b"), &no(), |n| bytes += n).unwrap();
        assert_eq!(bytes, 8);
        assert_eq!(fs::read_to_string(f.path("b/file")).unwrap(), "contents");
        assert!(a.exists());
    }
    #[test]
    fn copy_does_not_overwrite() {
        let f = Fixture::new();
        fs::write(f.path("a"), "new").unwrap();
        fs::write(f.path("b"), "old").unwrap();
        assert!(copy(&f.path("a"), &f.path("b"), &no(), |_| {}).is_err());
        assert_eq!(fs::read_to_string(f.path("b")).unwrap(), "old");
    }
    #[test]
    fn copy_does_not_merge_folders() {
        let f = Fixture::new();
        create(&f.0, "a", true).unwrap();
        create(&f.0, "b", true).unwrap();
        fs::write(f.path("b/keep"), "old").unwrap();
        assert!(copy(&f.path("a"), &f.path("b"), &no(), |_| {}).is_err());
        assert!(f.path("b/keep").exists());
    }
    #[test]
    fn copy_rejects_self_and_descendants() {
        let f = Fixture::new();
        let a = create(&f.0, "a", true).unwrap();
        assert!(copy(&a, &a, &no(), |_| {}).is_err());
        assert!(copy(&a, &a.join("nested"), &no(), |_| {}).is_err());
        assert!(!a.join("nested").exists());
    }
    #[test]
    fn copy_rejects_symlinked_descendants() {
        let f = Fixture::new();
        let a = create(&f.0, "a", true).unwrap();
        symlink(&a, f.path("alias")).unwrap();
        assert!(copy(&a, &f.path("alias/nested"), &no(), |_| {}).is_err());
    }
    #[test]
    fn cancelled_copy_preserves_source() {
        let f = Fixture::new();
        fs::write(f.path("a"), "source").unwrap();
        assert!(copy(&f.path("a"), &f.path("b"), &AtomicBool::new(true), |_| {}).is_err());
        assert!(!f.path("b").exists());
        assert!(f.path("a").exists());
    }
    #[test]
    fn partial_cancel_removes_only_new_destination() {
        let f = Fixture::new();
        fs::write(f.path("a"), vec![1u8; 3 * 1024 * 1024]).unwrap();
        let cancel = no();
        assert!(
            copy(&f.path("a"), &f.path("b"), &cancel, |_| cancel
                .store(true, AtomicOrdering::Relaxed))
            .is_err()
        );
        assert!(!f.path("b").exists());
        assert_eq!(fs::metadata(f.path("a")).unwrap().len(), 3 * 1024 * 1024);
    }
    #[test]
    fn symlinks_are_copied_without_following() {
        let f = Fixture::new();
        symlink("missing", f.path("link")).unwrap();
        copy(&f.path("link"), &f.path("copied"), &no(), |_| {}).unwrap();
        assert_eq!(
            fs::read_link(f.path("copied")).unwrap(),
            Path::new("missing")
        );
    }
    #[test]
    fn move_is_exclusive() {
        let f = Fixture::new();
        fs::write(f.path("a"), "a").unwrap();
        fs::write(f.path("b"), "b").unwrap();
        assert!(move_item(&f.path("a"), &f.path("b"), &no(), |_| {}).is_err());
        assert_eq!(fs::read_to_string(f.path("a")).unwrap(), "a");
        assert_eq!(fs::read_to_string(f.path("b")).unwrap(), "b");
    }
    #[test]
    fn successful_move_and_identity() {
        let f = Fixture::new();
        fs::write(f.path("a"), "a").unwrap();
        let id = Identity::read(&f.path("a")).unwrap();
        move_item(&f.path("a"), &f.path("b"), &no(), |_| {}).unwrap();
        assert!(!f.path("a").exists());
        assert!(id.matches(&f.path("b")));
    }
    #[test]
    fn rename_is_exclusive() {
        let f = Fixture::new();
        create(&f.0, "a", false).unwrap();
        create(&f.0, "b", false).unwrap();
        assert!(rename(&f.path("a"), "b").is_err());
        assert!(f.path("a").exists());
    }
    #[test]
    fn unique_names_respect_extensions_and_dangling_links() {
        let f = Fixture::new();
        symlink("missing", f.path("a.txt")).unwrap();
        assert_eq!(
            unique_path(&f.0, std::ffi::OsStr::new("a.txt"), &no()).unwrap(),
            f.path("a copy.txt")
        );
        create(&f.0, "a copy.txt", false).unwrap();
        assert_eq!(
            unique_path(&f.0, std::ffi::OsStr::new("a.txt"), &no()).unwrap(),
            f.path("a copy 2.txt")
        );
    }
    #[test]
    fn search_is_recursive_case_insensitive_and_skips_link_loops() {
        let f = Fixture::new();
        create(&f.0, "nested", true).unwrap();
        create(&f.path("nested"), "Report.txt", false).unwrap();
        symlink(&f.0, f.path("nested/loop")).unwrap();
        let r = search(&f.0, "report", false, &no()).unwrap();
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].name, "Report.txt");
    }
    #[test]
    fn search_respects_hidden() {
        let f = Fixture::new();
        create(&f.0, ".hidden", true).unwrap();
        create(&f.path(".hidden"), "match", false).unwrap();
        assert!(
            search(&f.0, "match", false, &no())
                .unwrap()
                .entries
                .is_empty()
        );
        assert_eq!(search(&f.0, "match", true, &no()).unwrap().entries.len(), 1);
    }
    #[test]
    fn preview_bounds_reads_and_detects_binary() {
        let f = Fixture::new();
        fs::write(f.path("a"), "x".repeat(40000)).unwrap();
        assert!(preview_text(&f.path("a")).unwrap().len() < 34000);
        fs::write(f.path("b"), [1, 0, 2]).unwrap();
        assert!(preview_text(&f.path("b")).is_err());
    }
    #[test]
    fn formats_sizes_and_dates() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1500), "1.5 KB");
        assert_eq!(date(UNIX_EPOCH), "1970-01-01 00:00");
        assert_eq!(
            date(UNIX_EPOCH + std::time::Duration::from_secs(1709164800)),
            "2024-02-29 00:00"
        );
    }
    #[test]
    fn directory_symlinks_open_and_list_their_target_contents() {
        let f = Fixture::new();
        let target = create(&f.0, "target", true).unwrap();
        fs::write(target.join("inside.txt"), "inside").unwrap();
        symlink("target", f.path("folder link")).unwrap();
        let entry = Entry::read_for_open(f.path("folder link")).unwrap();
        assert!(entry.symlink && entry.navigable());
        let listing = list(&entry.path, false, &no()).unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].path, f.path("folder link/inside.txt"));
    }
    #[test]
    fn file_symlinks_are_opened_as_files() {
        let f = Fixture::new();
        fs::write(f.path("target.txt"), "contents").unwrap();
        symlink("target.txt", f.path("file link")).unwrap();
        let entry = Entry::read_for_open(f.path("file link")).unwrap();
        assert!(entry.symlink);
        assert!(!entry.navigable());
        assert_eq!(entry.bytes, 8);
    }
    #[test]
    fn broken_links_remain_visible_but_open_reports_missing_target() {
        let f = Fixture::new();
        symlink("missing", f.path("broken")).unwrap();
        assert!(Entry::read(f.path("broken")).unwrap().symlink);
        assert_eq!(
            Entry::read_for_open(f.path("broken")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
    #[test]
    fn links_to_application_bundles_are_launched_instead_of_browsed() {
        let f = Fixture::new();
        create(&f.0, "Example.APP", true).unwrap();
        symlink("Example.APP", f.path("app link")).unwrap();
        let entry = Entry::read_for_open(f.path("app link")).unwrap();
        assert!(entry.symlink && entry.application);
        assert!(!entry.navigable());
    }
}
