#!/bin/bash
set -euo pipefail

installer_directory="$(cd "$(dirname "$0")" && pwd)"
if [[ -d "$installer_directory/Explorer.app" ]]; then
  bundle="$installer_directory/Explorer.app"
else
  bundle="$installer_directory/../dist/Explorer.app"
fi
if [[ ! -d "$bundle" ]]; then
  printf 'Explorer.app is missing. Build it with scripts/bundle.sh first.\n' >&2
  exit 1
fi
/usr/bin/codesign --verify --deep --strict "$bundle"

install_directory="${EXPLORER_INSTALL_DIR:-/Applications}"
if [[ -z "${EXPLORER_INSTALL_DIR:-}" && ! -w /Applications ]]; then
  install_directory="$HOME/Applications"
fi
destination="$install_directory/Explorer.app"
if [[ -e "$destination" ]]; then
  installed_identifier="$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$destination/Contents/Info.plist" 2>/dev/null || true)"
  if [[ "$installed_identifier" != com.explorer.mac ]]; then
    printf 'Another app already occupies %s; installation stopped.\n' "$destination" >&2
    exit 1
  fi
fi

mkdir -p "$install_directory"
staging="$(mktemp -d "$install_directory/.explorer-install.XXXXXX")"
cleanup() {
  if [[ ! -e "$destination" && -d "$staging/previous.app" ]]; then
    mv "$staging/previous.app" "$destination"
  fi
  rm -rf "$staging"
}
trap cleanup EXIT
/usr/bin/ditto "$bundle" "$staging/Explorer.app"
/usr/bin/codesign --verify --deep --strict "$staging/Explorer.app"
if [[ -e "$destination" ]]; then
  mv "$destination" "$staging/previous.app"
fi
mv "$staging/Explorer.app" "$destination"
printf 'Installed Explorer at %s\n' "$destination"
if [[ "${EXPLORER_NO_LAUNCH:-0}" != 1 ]]; then
  /usr/bin/open -n "$destination"
fi
