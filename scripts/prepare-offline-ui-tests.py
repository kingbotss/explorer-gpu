"""Prepare GPUI's real headless event dispatcher using already-cached crates.

Offline-only adaptations: disable unused git utilities in test support and use
std::backtrace for diagnostic stack traces. No input, layout, rendering, or
application code is patched. The normal test-ui.sh path uses unmodified GPUI.
"""
from pathlib import Path
import shutil
root = Path(__file__).resolve().parent.parent
work = root / "target/ui-harness"
work.mkdir(parents=True, exist_ok=True)
sources = list((Path.home() / ".cargo/registry/src").glob("*/gpui-0.2.2"))
if not sources:
    raise SystemExit("Build the application once to cache gpui 0.2.2 first.")
gpui = work / "gpui"
if not gpui.exists():
    shutil.copytree(sources[0], gpui)
manifest = gpui / "Cargo.toml"
manifest.write_text(manifest.read_text().replace('    "util/test-support",\n', ''))
shim = work / "backtrace"
(shim / "src").mkdir(parents=True, exist_ok=True)
(shim / "Cargo.toml").write_text('[package]\nname="backtrace"\nversion="0.3.76"\nedition="2024"\n')
(shim / "src/lib.rs").write_text('''#[derive(Clone,Debug)]
#[allow(dead_code)]
pub struct Backtrace(std::sync::Arc<std::backtrace::Backtrace>);
impl Backtrace {
 pub fn new_unresolved()->Self{Self(std::sync::Arc::new(std::backtrace::Backtrace::force_capture()))}
 pub fn new()->Self{Self::new_unresolved()}
 pub fn resolve(&mut self){}
}
''')
(work / "Cargo.toml").write_text('''[package]
name="explorer-ui-tests"
version="0.1.0"
edition="2024"
[workspace]
[lib]
path="../../tests/gpui/harness.rs"
[dependencies]
gpui={version="=0.2.2",features=["runtime_shaders","test-support"]}
explorer={path="../.."}
anyhow="1"
[patch.crates-io]
gpui={path="gpui"}
backtrace={path="backtrace"}
''')
if not (work / "Cargo.lock").exists():
    shutil.copyfile(root / "Cargo.lock", work / "Cargo.lock")
