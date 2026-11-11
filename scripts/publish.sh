#!/bin/bash
# Publish without needing to write .git inside a restricted source workspace.
set -euo pipefail
cd "$(dirname "$0")/.."
source_dir="$PWD"
remote="${1:-https://github.com/kingbotss/explorer-gpu.git}"
publish_dir="$(mktemp -d "${TMPDIR:-/tmp}/explorer-publish.XXXXXX")"
printf 'Preparing checkout in %s\n' "$publish_dir"
# Clone existing history; never force-push or reset a remote branch.
git clone "$remote" "$publish_dir/repo"
if ! git -C "$publish_dir/repo" rev-parse --verify HEAD >/dev/null 2>&1; then
  git -C "$publish_dir/repo" symbolic-ref HEAD refs/heads/main
fi
rsync -a --exclude='.git' --exclude='target/' --exclude='dist/' --exclude='.DS_Store' --exclude='tests/gpui/Cargo.lock' "$source_dir/" "$publish_dir/repo/"
git -C "$publish_dir/repo" add .
if git -C "$publish_dir/repo" diff --cached --quiet; then
  printf 'The repository already contains these changes.\n'
  exit 0
fi
git -C "$publish_dir/repo" commit -m "Build native GPUI Explorer with Windows-style interface and tested file operations"
git -C "$publish_dir/repo" push -u origin HEAD
printf 'Published to %s\n' "$remote"
