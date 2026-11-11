//! Real filesystem integration tests. Built by scripts/test-offline.sh without GPUI.
#![allow(dead_code)]
#[path = "../src/jobs.rs"]
mod jobs;
#[path = "../src/native.rs"]
mod native;
use explorer::fs::{self, Change};
use jobs::{Job, JobEvent};
use std::{
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
            "explorer-integration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, s: &str) -> PathBuf {
        self.0.join(s)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(job: Job) -> (Vec<Change>, Vec<String>, Option<Vec<Change>>) {
    let (tx, rx) = mpsc::channel();
    jobs::run(job, Arc::new(AtomicBool::new(false)), tx);
    for e in rx {
        if let JobEvent::Done {
            changes,
            errors,
            remaining_undo,
            ..
        } = e
        {
            return (changes, errors, remaining_undo);
        }
    }
    panic!("Operation did not complete")
}
#[test]
fn job_copy_generates_nonconflicting_names() {
    let f = Fixture::new();
    std::fs::write(f.path("file.txt"), "original").unwrap();
    let (c, e, _) = run(Job::Transfer {
        paths: vec![f.path("file.txt")],
        destination: f.0.clone(),
        cut: false,
    });
    assert!(e.is_empty());
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].current, f.path("file copy.txt"));
    assert_eq!(
        std::fs::read_to_string(f.path("file.txt")).unwrap(),
        "original"
    );
}
#[test]
fn job_continues_after_missing_source() {
    let f = Fixture::new();
    std::fs::write(f.path("good"), "data").unwrap();
    std::fs::create_dir(f.path("dest")).unwrap();
    let (c, e, _) = run(Job::Transfer {
        paths: vec![f.path("missing"), f.path("good")],
        destination: f.path("dest"),
        cut: false,
    });
    assert_eq!(e.len(), 1);
    assert_eq!(c.len(), 1);
    assert_eq!(
        std::fs::read_to_string(f.path("dest/good")).unwrap(),
        "data"
    );
}
#[test]
fn rename_and_undo_roundtrip() {
    let f = Fixture::new();
    std::fs::write(f.path("before"), "data").unwrap();
    let (c, e, _) = run(Job::Rename {
        path: f.path("before"),
        name: "after".into(),
    });
    assert!(e.is_empty());
    let (_, e, r) = run(Job::Undo(c));
    assert!(e.is_empty());
    assert!(r.unwrap().is_empty());
    assert!(f.path("before").exists());
    assert!(!f.path("after").exists());
}
#[test]
fn undo_does_not_overwrite_recreated_original() {
    let f = Fixture::new();
    std::fs::write(f.path("before"), "old").unwrap();
    let (c, _, _) = run(Job::Rename {
        path: f.path("before"),
        name: "after".into(),
    });
    std::fs::write(f.path("before"), "new").unwrap();
    let (_, e, r) = run(Job::Undo(c));
    assert!(!e.is_empty());
    assert_eq!(r.unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(f.path("before")).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(f.path("after")).unwrap(), "old");
}
#[test]
fn undo_checks_identity() {
    let f = Fixture::new();
    std::fs::write(f.path("a"), "a").unwrap();
    let (c, _, _) = run(Job::Rename {
        path: f.path("a"),
        name: "b".into(),
    });
    std::fs::rename(f.path("b"), f.path("saved")).unwrap();
    std::fs::write(f.path("b"), "replacement").unwrap();
    let (_, e, r) = run(Job::Undo(c));
    assert!(!e.is_empty());
    assert_eq!(r.unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(f.path("b")).unwrap(), "replacement");
}
#[test]
fn native_zip_handles_spaces_quotes_and_unicode() {
    let f = Fixture::new();
    let source = f.path("hello ' 你好.txt");
    std::fs::write(&source, "zip contents").unwrap();
    let zip = f.path("result.zip");
    native::compress(&source, &zip).unwrap();
    let out = std::process::Command::new("/usr/bin/unzip")
        .arg("-p")
        .arg(&zip)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "zip contents");
}
#[test]
fn native_zip_never_overwrites() {
    let f = Fixture::new();
    std::fs::write(f.path("a"), "data").unwrap();
    std::fs::write(f.path("archive.zip"), "keep").unwrap();
    assert!(native::compress(&f.path("a"), &f.path("archive.zip")).is_err());
    assert_eq!(
        std::fs::read_to_string(f.path("archive.zip")).unwrap(),
        "keep"
    );
}
#[test]
fn session_save_load_roundtrip() {
    use explorer::session::Session;
    let f = Fixture::new();
    let s = Session {
        tabs: vec![f.path("你好\nfolder")],
        favorites: vec![f.path("space folder")],
        dark: true,
        preview: true,
        ..Default::default()
    };
    s.save(&f.path("config/session")).unwrap();
    let r = Session::load(&f.path("config/session"));
    assert_eq!(r.tabs, s.tabs);
    assert_eq!(r.favorites, s.favorites);
    assert!(r.dark && r.preview);
}
#[test]
fn move_job_keeps_source_on_invalid_destination() {
    let f = Fixture::new();
    std::fs::write(f.path("a"), "data").unwrap();
    let (c, e, _) = run(Job::Transfer {
        paths: vec![f.path("a")],
        destination: f.path("missing"),
        cut: true,
    });
    assert!(c.is_empty());
    assert!(!e.is_empty());
    assert_eq!(std::fs::read_to_string(f.path("a")).unwrap(), "data");
}
#[test]
fn creates_do_not_replace_existing_items() {
    let f = Fixture::new();
    fs::create(&f.0, "folder", true).unwrap();
    let (c, e, _) = run(Job::Create {
        parent: f.0.clone(),
        name: "folder".into(),
        directory: true,
    });
    assert!(c.is_empty());
    assert!(!e.is_empty());
    assert!(f.path("folder").is_dir());
}

fn write_test_bmp(path: &std::path::Path) {
    let mut bytes = vec![0u8; 58];
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&58u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&4u32.to_le_bytes());
    bytes[56] = 255;
    std::fs::write(path, bytes).unwrap();
}
fn write_test_pdf(path: &std::path::Path) {
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, body) in [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>",
    ]
    .iter()
    .enumerate()
    {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, body).as_bytes());
    }
    let xref = data.len();
    data.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    std::fs::write(path, data).unwrap();
}
#[test]
fn native_image_decoder_reads_valid_image() {
    let f = Fixture::new();
    write_test_bmp(&f.path("image.bmp"));
    assert_eq!(
        native::probe_media(&f.path("image.bmp"), explorer::media::MediaKind::Image).unwrap(),
        1
    );
}
#[test]
fn native_image_decoder_rejects_invalid_data() {
    let f = Fixture::new();
    std::fs::write(f.path("bad.png"), "not an image").unwrap();
    assert!(native::probe_media(&f.path("bad.png"), explorer::media::MediaKind::Image).is_err());
}
#[test]
fn native_image_decoder_follows_symlink() {
    let f = Fixture::new();
    write_test_bmp(&f.path("target.bmp"));
    std::os::unix::fs::symlink("target.bmp", f.path("link")).unwrap();
    assert_eq!(
        native::probe_media(&f.path("link"), explorer::media::MediaKind::Image).unwrap(),
        1
    );
}
#[test]
fn native_pdf_decoder_reads_page_count() {
    let f = Fixture::new();
    write_test_pdf(&f.path("document.pdf"));
    assert_eq!(
        native::probe_media(&f.path("document.pdf"), explorer::media::MediaKind::Pdf).unwrap(),
        1
    );
}
#[test]
fn native_pdf_decoder_rejects_invalid_data() {
    let f = Fixture::new();
    std::fs::write(f.path("bad.pdf"), "not a PDF").unwrap();
    assert!(native::probe_media(&f.path("bad.pdf"), explorer::media::MediaKind::Pdf).is_err());
}
