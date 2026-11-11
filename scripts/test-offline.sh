#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target/offline
rustc --edition=2024 --test src/lib.rs -o target/offline/core-tests
target/offline/core-tests
clang -fobjc-arc -fmodules -fmodules-cache-path="$PWD/target/offline/modules" -mmacosx-version-min=12.0 -c native/macos.m -o target/offline/macos.o
ar rcs target/offline/libexplorer_native.a target/offline/macos.o
rustc --edition=2024 --crate-name explorer --crate-type rlib src/lib.rs -o target/offline/libexplorer.rlib
rustc --edition=2024 --test tests/operations.rs --extern explorer=target/offline/libexplorer.rlib -L native=target/offline -l static=explorer_native -l framework=AppKit -l framework=Foundation -o target/offline/operation-tests
target/offline/operation-tests
