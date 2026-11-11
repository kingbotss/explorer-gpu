//! Classify local media without changing how folders or application bundles open.
use std::path::Path;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum MediaKind {
    Image = 1,
    Video = 2,
    Pdf = 3,
}
fn extension(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "heic" | "heif" | "webp"
        | "ico" | "icns" | "svg" => Some(MediaKind::Image),
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" => Some(MediaKind::Video),
        "pdf" => Some(MediaKind::Pdf),
        _ => None,
    }
}
pub fn kind(path: &Path) -> Option<MediaKind> {
    std::fs::canonicalize(path)
        .ok()
        .and_then(|p| extension(&p))
        .or_else(|| extension(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_images_case_insensitively() {
        for p in ["photo.JPG", "image.heic", "pic.webp", "animation.gif"] {
            assert_eq!(kind(Path::new(p)), Some(MediaKind::Image));
        }
    }
    #[test]
    fn classifies_video_containers() {
        for p in ["movie.mp4", "clip.MOV", "video.m4v", "recording.mkv"] {
            assert_eq!(kind(Path::new(p)), Some(MediaKind::Video));
        }
    }
    #[test]
    fn pdf_and_other_files() {
        assert_eq!(kind(Path::new("document.PDF")), Some(MediaKind::Pdf));
        assert_eq!(kind(Path::new("notes.txt")), None);
        assert_eq!(kind(Path::new("data")), None);
    }
    #[test]
    fn extensionless_symlink_uses_target_kind() {
        let root = std::env::temp_dir().join(format!("explorer-media-kind-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("photo.jpg"), b"test").unwrap();
        std::os::unix::fs::symlink("photo.jpg", root.join("link")).unwrap();
        assert_eq!(kind(&root.join("link")), Some(MediaKind::Image));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn misleading_symlink_name_uses_target_kind() {
        let root =
            std::env::temp_dir().join(format!("explorer-media-link-kind-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("photo.jpg"), b"test").unwrap();
        std::os::unix::fs::symlink("photo.jpg", root.join("link.pdf")).unwrap();
        assert_eq!(kind(&root.join("link.pdf")), Some(MediaKind::Image));
        let _ = std::fs::remove_dir_all(root);
    }
}
