#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "${1:-}" == "--offline" ]]; then
  python3 scripts/prepare-offline-ui-tests.py
  cargo test --manifest-path target/ui-harness/Cargo.toml --offline -- --nocapture
else
  CARGO_TARGET_DIR="$PWD/target/ui-tests" cargo test --manifest-path tests/gpui/Cargo.toml -- --nocapture
fi
