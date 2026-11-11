use std::{env, path::PathBuf, process::Command};
fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        panic!("Explorer currently targets macOS.");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut objects = Vec::new();
    for name in ["macos", "media"] {
        let source = format!("native/{name}.m");
        println!("cargo:rerun-if-changed={source}");
        let object = out.join(format!("{name}.o"));
        let status = Command::new("clang")
            .arg(format!(
                "-fmodules-cache-path={}",
                out.join("modules").display()
            ))
            .args(["-fobjc-arc", "-fmodules", "-mmacosx-version-min=12.0", "-c"])
            .arg(source)
            .arg("-o")
            .arg(&object)
            .status()
            .expect("Install Xcode command-line tools");
        assert!(status.success(), "Native bridge compilation failed");
        objects.push(object);
    }
    assert!(
        Command::new("ar")
            .arg("rcs")
            .arg(out.join("libexplorer_native.a"))
            .args(objects)
            .status()
            .unwrap()
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=explorer_native");
    for framework in [
        "AppKit",
        "Foundation",
        "AVKit",
        "AVFoundation",
        "PDFKit",
        "CoreMedia",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
