use std::{
    ffi::{CStr, CString, c_char},
    io,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
unsafe extern "C" {
    fn explorer_free(value: *mut c_char);
    fn explorer_double_click_interval() -> f64;
    fn explorer_trash(path: *const c_char, error: *mut *mut c_char) -> *mut c_char;
    fn explorer_clipboard_write(paths: *const *const c_char, count: usize) -> i64;
    fn explorer_clipboard_read() -> *mut c_char;
    fn explorer_clipboard_version() -> i64;
    fn explorer_open_media(path: *const c_char, kind: i32, error: *mut *mut c_char) -> i32;
    fn explorer_viewer_count() -> usize;
    fn explorer_set_main_window_count(count: usize);
    #[cfg(test)]
    #[allow(dead_code)]
    fn explorer_probe_media(path: *const c_char, kind: i32, error: *mut *mut c_char) -> usize;
}
pub fn set_main_window_count(count: usize) {
    unsafe { explorer_set_main_window_count(count) }
}
pub fn viewer_count() -> usize {
    unsafe { explorer_viewer_count() }
}
pub fn open_media(path: &Path, kind: explorer::media::MediaKind) -> io::Result<()> {
    let path = cpath(path)?;
    let mut error = std::ptr::null_mut();
    if unsafe { explorer_open_media(path.as_ptr(), kind as i32, &mut error) } != 0 {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&unsafe { take(error) }).into_owned(),
        ))
    }
}
#[cfg(test)]
#[allow(dead_code)]
pub fn probe_media(path: &Path, kind: explorer::media::MediaKind) -> io::Result<usize> {
    let path = cpath(path)?;
    let mut error = std::ptr::null_mut();
    let result = unsafe { explorer_probe_media(path.as_ptr(), kind as i32, &mut error) };
    if result > 0 {
        Ok(result)
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&unsafe { take(error) }).into_owned(),
        ))
    }
}
pub fn double_click_interval() -> std::time::Duration {
    let seconds = unsafe { explorer_double_click_interval() };
    std::time::Duration::from_secs_f64(if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.5
    })
}
fn cpath(path: &Path) -> io::Result<CString> {
    Ok(CString::new(path.as_os_str().as_bytes())?)
}
unsafe fn take(ptr: *mut c_char) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: All bridge strings are nul-terminated malloc allocations.
    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec();
    unsafe { explorer_free(ptr) };
    bytes
}
pub fn trash(path: &Path) -> io::Result<PathBuf> {
    let p = cpath(path)?;
    let mut error = std::ptr::null_mut();
    let result = unsafe { explorer_trash(p.as_ptr(), &mut error) };
    if result.is_null() {
        Err(io::Error::other(
            String::from_utf8_lossy(&unsafe { take(error) }).into_owned(),
        ))
    } else {
        Ok(std::ffi::OsString::from_vec(unsafe { take(result) }).into())
    }
}
pub fn clipboard_write(paths: &[PathBuf]) -> io::Result<i64> {
    let strings: Vec<_> = paths.iter().map(|p| cpath(p)).collect::<io::Result<_>>()?;
    let pointers: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
    Ok(unsafe { explorer_clipboard_write(pointers.as_ptr(), pointers.len()) })
}
pub fn clipboard_version() -> i64 {
    unsafe { explorer_clipboard_version() }
}
pub fn clipboard_read() -> Vec<PathBuf> {
    let text = unsafe { take(explorer_clipboard_read()) };
    String::from_utf8_lossy(&text)
        .lines()
        .filter_map(|line| {
            if !line.is_ascii() || !line.len().is_multiple_of(2) {
                return None;
            }
            let bytes: Option<Vec<_>> = (0..line.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&line[i..i + 2], 16).ok())
                .collect();
            Some(PathBuf::from(std::ffi::OsString::from_vec(bytes?)))
        })
        .collect()
}
fn launch(program: &str, args: &[&std::ffi::OsStr]) -> io::Result<()> {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut child| {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
}
pub fn open(path: &Path) -> io::Result<()> {
    launch("/usr/bin/open", &[path.as_os_str()])
}
pub fn reveal(path: &Path) -> io::Result<()> {
    launch("/usr/bin/open", &["-R".as_ref(), path.as_os_str()])
}
pub fn quick_look(paths: &[PathBuf]) -> io::Result<()> {
    let mut cmd = Command::new("/usr/bin/qlmanage");
    cmd.arg("-p")
        .args(paths)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub fn terminal_directory(path: &Path) -> io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)?;
    if resolved.is_dir() {
        Ok(resolved)
    } else {
        resolved
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| io::Error::other("Missing containing folder"))
    }
}
pub fn terminal(path: &Path) -> io::Result<PathBuf> {
    let directory = terminal_directory(path)?;
    // Run on the caller's worker thread; report Launch Services failures rather
    // than treating a successful spawn of /usr/bin/open as a successful launch.
    // Pass the folder as an argv value, never as a shell or AppleScript command.
    let result = Command::new("/usr/bin/open")
        .args(["-a", "/System/Applications/Utilities/Terminal.app"])
        .arg(&directory)
        .output()?;
    if result.status.success() {
        Ok(directory)
    } else {
        let detail = String::from_utf8_lossy(&result.stderr);
        Err(io::Error::other(format!(
            "Cannot open Terminal in {}: {}",
            directory.display(),
            if detail.trim().is_empty() {
                result.status.to_string()
            } else {
                detail.trim().to_owned()
            }
        )))
    }
}
pub fn compress(path: &Path, target: &Path) -> io::Result<()> {
    // Reserve a private staging folder: ditto cannot overwrite any existing user file.
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("Missing archive destination"))?;
    let mut n = 0u64;
    let staging = loop {
        let p = parent.join(format!(".explorer-zip-{}-{n}", std::process::id()));
        match std::fs::create_dir(&p) {
            Ok(()) => break p,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    let archive = staging.join("archive.zip");
    let result = (|| {
        let out = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
            .arg(path)
            .arg(&archive)
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        explorer::fs::rename_exclusive(&archive, target)
    })();
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir(staging);
    result
}
pub fn extract(path: &Path) -> io::Result<()> {
    // Archive Utility owns ZIP extraction, including its macOS path/symlink safety policy.
    // Explicitly open in Archive Utility rather than the user's possibly unrelated .zip app.
    launch(
        "/usr/bin/open",
        &["-a".as_ref(), "Archive Utility".as_ref(), path.as_os_str()],
    )
}
