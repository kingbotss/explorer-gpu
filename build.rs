use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=native/macos.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        panic!("Explorer currently targets macOS.");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut clang = Command::new("clang");
    clang.arg(format!(
        "-fmodules-cache-path={}",
        out.join("modules").display()
    ));
    clang
        .args([
            "-fobjc-arc",
            "-fmodules",
            "-mmacosx-version-min=12.0",
            "-c",
            "native/macos.m",
            "-o",
        ])
        .arg(out.join("macos.o"));
    assert!(
        clang
            .status()
            .expect("Install Xcode command-line tools")
            .success(),
        "Native bridge compilation failed"
    );
    assert!(
        Command::new("ar")
            .arg("rcs")
            .arg(out.join("libexplorer_native.a"))
            .arg(out.join("macos.o"))
            .status()
            .unwrap()
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=explorer_native");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
}
