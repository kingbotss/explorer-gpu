# Explorer for macOS

A native Rust + GPUI file manager with a Windows 11 Explorer-style interface: neutral surfaces, compact tabs and command bar, yellow folder icons, a navigation tree, details/grid views, and an optional preview pane. GPUI renders the interface directly; there is no browser or webview.

**Status: initial implementation.** The full GPUI application passes `cargo check --all-targets` and builds in release mode. The 72 core/native-operation tests and 31 rendered GPUI event tests pass, and the signed app bundle is generated at `dist/Explorer.app`. The live input log exposed a 150 ms macOS double-click setting rejecting recorded 167–200 ms click pairs; replaying the timing in a rendered test reproduced the failure. Version 0.3.3 adds a 500 ms application minimum while preserving slower system preferences. The running 0.3.3 app's log subsequently confirmed folder activation and navigation from paired presses reported as single clicks. Full Windows Explorer/Finder feature parity is not claimed.

## Run

Version 0.3.4 places folder-size recalculation in Properties rather than the right-click menu. Folder sizes still calculate automatically when selected or opened in Properties; View → Calculate all folder sizes measures the listing, and sorting by Size also starts measurement. Folder presses pair using at least 500 ms, or the system interval if longer, independently of GPUI's click/drag callbacks. A bounded input log records the configured/effective intervals, actual event counts, and activation decisions. Quit any running copy and reopen `dist/Explorer.app` to load this version; the title bar and About Explorer show the version.

Requires macOS 12+, a recent stable Rust toolchain (edition 2024), Xcode command-line tools, and internet access for the first dependency download. GPUI is pinned to 0.2.2 with its `runtime_shaders` feature: macOS compiles the embedded Metal shaders at launch, so the separate Xcode Metal compiler download is not required.

```sh
cargo run --release
# Start in a particular folder:
cargo run --release -- ~/Downloads
```

The first build compiles GPUI and its dependencies and can take several minutes. Cargo.lock is included for reproducible builds. If necessary, select Xcode with `xcode-select`.

Create an app bundle:

```sh
./scripts/bundle.sh
open dist/Explorer.app
```

The bundle script builds a release binary, generates a Retina app icon, includes privacy descriptions, and applies an ad-hoc local signature. Distribution signing and notarization are not configured. It does not install or replace Finder.

## Implemented in the source

- Independent tabs with back/forward history; breadcrumbs, editable paths, `~` expansion, and parent navigation.
- Expandable folder tree, common macOS locations, mounted volumes, iCloud Drive when present, and persistent folder pins.
- Virtualized details and four-column grid views; natural filename sorting; sortable name/date/kind/size columns; folders first; hidden-file toggle.
- Single, Command-click, Shift-click, keyboard, and select-all selection.
- A capture-phase handler checks the visible, clipped row bounds and opens on the second press. It accepts native double-click counts and independently pairs synthetic presses within at least 500 ms (longer system preferences are honored), even without completed-click callbacks. Double-click opening takes precedence over selection modifiers; a single click still selects or toggles selection. Fn does not block opening. Clicks stay keyed to a folder path across re-renders. Refreshes wait until a click sequence ends, and eight pixels of deliberate movement are required to begin dragging. Folder symlinks are browsable, file/application symlinks open with macOS, and broken links report an error.
- Recursive folder sizes appear in the Size column, grid, preview, Properties, and selected-item totals. One background worker measures selected folders; Properties offers recalculation and cancellation, and View can measure the whole listing. Sorting by Size starts measurement and delays row movement until the current click sequence ends. View → Cancel size calculation stops pending work, and navigating away cancels it automatically. Hidden files count; internal symlinks contribute their own link length and are never traversed. A selected root symlink resolves to its folder target. Totals use logical file lengths, count hard links per directory entry, and exclude directory metadata. Partial results show unreadable entries; explicit Refresh discards cached measurements.
- Immediate filename filtering; Enter in search performs a cancellable recursive search with a 10,000-result cap. Directory symlinks are not traversed.
- New folders and empty files; rename; duplicate; recursive copy and move, including cross-volume moves.
- File URL clipboard interoperability with Finder, plus an internal cut clipboard.
- Internal drag-and-drop copying to folders/sidebar locations, and files dropped from Finder. Dragging out into another app is not implemented.
- Native macOS Trash with confirmation. The application has no permanent-delete command.
- Session-local undo for successful creates, copies, renames, moves, compression, and Trash operations. Undo verifies file identity and never overwrites a recreated original. Undoing creation/copy moves the result to Trash.
- ZIP compression through macOS `ditto`, staged before exclusive publication. ZIP extraction opens Archive Utility.
- In-app media windows on double-click or Enter, backed by macOS frameworks in the same process. Image files get zoom, fit, Retina-aware actual size, pinch/pan, GIF animation, and previous/next image navigation. Videos get native play/pause, scrubbing, volume, frame stepping, full-screen controls, ten-second skips, and looping. PDFs get continuous scrolling, page navigation, zoom/fit, text selection/copy, search, and a password prompt for locked documents.
- Open externally in the context menu or media toolbar, plus Quick Look, Finder reveal, and Terminal integration. Closing a video window pauses playback; viewer windows can stay open after the file-manager window closes.
- The Terminal command opens the selected folder, a selected file's containing folder, or the current folder when there is no single selection. The background menu always opens the current folder. Folder symlinks resolve to their target. Launching runs on a worker thread and reports failures; paths are passed as arguments without shell interpolation.
- File properties, image/text preview, operation status and cancellation, light/dark themes, and saved tabs/preferences.
- Background directory loading/search/transfers. The active folder refreshes every three seconds when idle. Stale results from earlier navigations are discarded.
- Native text input integration with UTF-16/UTF-8 conversion, IME composition, selection, and text copy/paste.

Existing destinations are never silently overwritten or merged. Paste collisions get a `copy` suffix. Rename conflicts are reported. Copies preserve ordinary file permissions and modification times; ACLs, extended attributes, resource forks, hard-link relationships, APFS clones, and sparse allocation are not yet preserved by the copy engine. ZIP compression uses macOS metadata-aware `ditto`.

## Keyboard shortcuts

| Action | Shortcut |
| --- | --- |
| New / close tab | Command T / Command W |
| Go to path / search | Command L / Command F |
| Search subfolders | Enter in search field |
| Back / forward / up | Command [ / Command ] / Command Up |
| Open / Quick Look | Enter / Space |
| Copy / cut / paste | Command C / Command X / Command V |
| New folder / rename | Shift Command N / F2 |
| Duplicate / undo file operation | Command D / Command Z |
| Move to Trash | Command Backspace |
| Properties / refresh | Command I / Command R |
| Details / grid | Command 1 / Command 2 |
| Hidden files / preview pane | Shift Command . / Shift Command P |

## Verification

Run the core and native operation tests without downloading any crates:

```sh
./scripts/test-offline.sh
```

The core and native tests create isolated temporary directories. They cover exclusive creation/copy/move/rename, cancellation cleanup, descendant-copy rejection, symlink preservation and loops, search, natural sorting, Unicode text and settings, partial batch failures, undo conflicts and identity checks, real ZIP compression, media classification, and valid/invalid image and PDF decoding through the actual native loaders. They do not exercise a live macOS window, video playback, system clipboard, native Trash, or Quick Look.

With dependency access, also run:

```sh
cargo check --all-targets
cargo test
cargo run
```

The rendered-row regression tests exercise real GPUI layout and event dispatch without a WindowServer connection:

```sh
./scripts/test-ui.sh
# In a restricted environment with cached dependencies:
./scripts/test-ui.sh --offline
```

These tests reproduce the old Fn-flag failure and cover list/grid activation, native/synthetic click counts, interrupted releases, press pairs without click completions, icon-to-label movement, single-click selection, already-selected and multiply-selected folders, selection modifiers, Enter, folder symlinks, pointer jitter, Terminal availability/target selection, asynchronous folder measurement, size sorting, cancellation/stale results, and New/Sort/View menu actions. The ordinary test command uses unmodified GPUI test support. Offline preparation uses the cached GPUI source and replaces only diagnostic backtraces with `std::backtrace`, while disabling unused Git utilities; input, layout, rendering, and application code are unchanged. Offline adapters stay under ignored `target/` and are never linked into the app. Tests do not write the user's saved preferences or input log.

Before a release, manually validate the window on macOS: tabs/history, multi-selection, internal and Finder file drops, clipboard interoperability, IME, previews, permission-denied folders, native Trash/undo, large transfers/cancellation, image zoom/navigation, video playback/seek/loop/full screen, PDF search/unlock/text copy, and window closure during an operation. These interactive checks are outstanding.

## Current limits

This is a substantial first version, not complete Windows Explorer parity. Windows-style labels are adapted to the Mac: This PC browses `/`, Network & volumes browses `/Volumes`, and Recycle Bin uses macOS Trash. Missing features include editable permissions/ACLs, Finder tags, batch rename, column resizing, content/Spotlight search, thumbnail caching, native drag export, direct SMB/SFTP connections, a merge/replace conflict dialog, persistent undo, and crash-resumable transfers. Mounted network volumes can be browsed through `/Volumes`. macOS controls access to protected folders; inaccessible items are reported.

Media decoding and playback use the formats/codecs available in macOS; unsupported or damaged media reports an error, and Open externally remains available. Native media window controls and live playback still require interactive validation. Dates are shown in UTC. Text previews read at most 32 KB. Text cursor movement follows Unicode scalar boundaries rather than full grapheme clusters. ZIP extraction is delegated to Archive Utility and is not tracked in the application's undo stack. Cancellation is cooperative; compression and native Trash finish their current item before stopping. Normal app closure waits for the active worker to clean up; force-killing a process may leave an incomplete copy. If removal of a source fails during a cross-volume move, the completed destination copy is retained and an error is shown.

Preferences live at `~/Library/Application Support/Explorer/session`. Tabs, pins, theme, hidden-file visibility, view mode, and preview-pane visibility are saved. Navigation history and the undo stack last for the current session.

For the ongoing double-click investigation, `~/Library/Application Support/Explorer/input.log` records version/PID, root mouse-down events, row activation decisions, paths opened, navigation, and errors. It rotates at 128 KiB into `input.previous.log`, stays local, and is disabled in tests. If opening still fails, the log distinguishes missing platform events, row dispatch, and filesystem failures.

## Project structure

- `src/app.rs`: GPUI views, navigation, menus, selection, async event routing.
- `src/input.rs`, `src/text.rs`: native text input and Unicode range handling.
- `src/interaction.rs`: native/synthetic click activation and pointer/drag regression tests.
- `src/fs.rs`: dependency-free filesystem engine and unit tests.
- `src/jobs.rs`: background operations and undo bookkeeping.
- `src/native.rs`, `native/macos.m`: macOS integrations and file clipboard bridge.
- `src/media.rs`, `native/media.m`: media classification and native image, AVKit video, and PDFKit viewer windows.
- `src/session.rs`: bounded, atomic preferences storage, including non-UTF-8 paths.
- `src/icons.rs`: embedded SVG icon assets.
- `tests/operations.rs`: native filesystem integration tests.
- `tests/gpui/`: rendered GPUI event regression suite.
- `.github/workflows/macos.yml`: macOS checks, tests, and a downloadable app archive.

## GitHub

The requested destination is [kingbotss/explorer-gpu](https://github.com/kingbotss/explorer-gpu).

To publish from a Terminal with GitHub authentication and network access:

```sh
./scripts/publish.sh
```

The script clones existing remote history into a temporary checkout, overlays the source, commits changes, and pushes the checked-out branch. It never force-pushes or needs to write `.git` inside this source workspace. Generated binaries, build caches, and offline test adapters are excluded.

GPUI API reference: [gpui 0.2.2](https://docs.rs/gpui/0.2.2/gpui/).
