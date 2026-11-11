use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub tabs: Vec<PathBuf>,
    pub active: usize,
    pub favorites: Vec<PathBuf>,
    pub hidden: bool,
    pub grid: bool,
    pub preview: bool,
    pub dark: bool,
}
impl Session {
    pub fn location() -> PathBuf {
        home().join("Library/Application Support/Explorer/session")
    }
    pub fn load(path: &Path) -> Self {
        let mut text = String::new();
        if fs::File::open(path)
            .and_then(|f| f.take(65536).read_to_string(&mut text))
            .is_err()
        {
            return Self::default();
        }
        Self::decode(&text)
    }
    fn decode(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(' ') else {
                continue;
            };
            match key {
                "tab" if s.tabs.len() < 64 => {
                    if let Some(p) = decode_path(value) {
                        if p.is_absolute() {
                            s.tabs.push(p);
                        }
                    }
                }
                "pin" if s.favorites.len() < 128 => {
                    if let Some(p) = decode_path(value) {
                        if p.is_absolute() && !s.favorites.contains(&p) {
                            s.favorites.push(p);
                        }
                    }
                }
                "active" => s.active = value.parse().unwrap_or(0),
                "hidden" => s.hidden = value == "1",
                "grid" => s.grid = value == "1",
                "preview" => s.preview = value == "1",
                "dark" => s.dark = value == "1",
                _ => {}
            }
        }
        s.active = s.active.min(s.tabs.len().saturating_sub(1));
        s
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing settings directory"))?;
        fs::create_dir_all(parent)?;
        let mut text = format!(
            "explorer-session 1\nactive {}\nhidden {}\ngrid {}\npreview {}\ndark {}\n",
            self.active,
            u8::from(self.hidden),
            u8::from(self.grid),
            u8::from(self.preview),
            u8::from(self.dark)
        );
        for p in self.tabs.iter().take(64) {
            text.push_str(&format!("tab {}\n", encode_path(p)));
        }
        for p in self.favorites.iter().take(128) {
            text.push_str(&format!("pin {}\n", encode_path(p)));
        }
        let tmp = parent.join(format!(
            ".session-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let result = (|| {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
}
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
pub fn expand_path(text: &str, current: &Path) -> PathBuf {
    let text = text.trim();
    if text == "~" {
        home()
    } else if let Some(tail) = text.strip_prefix("~/") {
        home().join(tail)
    } else {
        let p = PathBuf::from(text);
        if p.is_absolute() { p } else { current.join(p) }
    }
}
pub(crate) fn encode_path(p: &Path) -> String {
    p.as_os_str()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn decode_path(s: &str) -> Option<PathBuf> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect();
    let bytes = bytes?;
    if bytes.contains(&0) {
        return None;
    }
    Some(std::ffi::OsString::from_vec(bytes).into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_unicode_newlines_and_raw_paths() {
        for p in [
            PathBuf::from("/tmp/hello\nworld 你好"),
            PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/\xff".to_vec())),
        ] {
            assert_eq!(decode_path(&encode_path(&p)), Some(p));
        }
    }
    #[test]
    fn corrupt_settings_are_tolerated() {
        let s = Session::decode("active 999\ntab nonsense\ntab 2f746d70\npin 00\ndark 1");
        assert_eq!(s.tabs, [PathBuf::from("/tmp")]);
        assert_eq!(s.active, 0);
        assert!(s.favorites.is_empty());
        assert!(s.dark);
    }
    #[test]
    fn relative_settings_paths_are_rejected() {
        assert!(Session::decode("tab 68656c6c6f").tabs.is_empty());
    }
    #[test]
    fn relative_navigation_uses_current_directory() {
        assert_eq!(
            expand_path("folder", Path::new("/tmp")),
            PathBuf::from("/tmp/folder")
        );
    }
}
