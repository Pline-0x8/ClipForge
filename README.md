# ClipForge

A Tauri desktop picker backed by a Rust clipboard service: 26 letter registers and a ring of 100 recent texts. It uses the host clipboard and native Copy/Paste commands. VMware/VM tools provide guest clipboard sharing.

The application name is **ClipForge**. Rust package names, executable names, and internal identifiers use lowercase `clipforge`.

## Run

Windows needs a recent stable Rust toolchain, MSVC build tools, and Microsoft WebView2 Runtime. The frontend is plain HTML/CSS/JavaScript; Node is needed only for frontend tests, and no npm install or frontend build is required.

```powershell
cargo build --release --locked
.\target\release\clipforge.exe
```

ClipForge starts hidden with no taskbar button. Press **Ctrl+Alt+Space** to toggle the centered menu. It stays open when modifiers are released and hides when you click outside it or switch applications. An active edit saves before hiding; validation or write failures keep the edit available. Clicking outside leaves focus with the application you clicked. Escape or Hide closes it; toggling it again saves any active edit before closing. Use **Quit** to exit. `clipforge.exe --show` opens the menu explicitly.

On Windows, Space uses `RegisterHotKey` with repeat suppression; the C/V register prefixes use the separate keyboard hook. Idle menu closing runs directly in Rust. A shortcut registration failure opens the menu with the error instead of leaving the app silently hidden. ClipForge does not create log files.

| Keys | Action |
| --- | --- |
| Ctrl+Alt+C, release C, then a-z | Native Copy into that register (Windows). |
| Ctrl+Alt+V, release V, then a-z | Load that register and trigger native Paste (Windows). |
| Ctrl+Alt+Space | Toggle the centered clipboard editor. |
| Arrows / Tab / Shift+Tab | Navigate menu entries. |
| Enter | Load the highlighted entry and close the menu. |
| Esc | Cancel an active edit; otherwise close the menu. |
| Ordinary Ctrl+C / Ctrl+V | Normal application behavior. |

Registers are case-insensitive. The letter prefix expires after two seconds. C/V never open the menu, however long they are held. You can release Ctrl/Alt before typing the letter; release all modifiers to allow native Copy/Paste injection. In a visible menu, a plain letter loads and pastes its register. Applications using Ctrl+Shift+C/V can use native Copy followed by **Save current** instead.

## Picker and history

- **Current Clipboard** at the top shows the host clipboard's current text. Drag a history entry onto it to load that full text, or click the cell to edit the clipboard manually. You can also drag current text into a register.
- Click a populated register cell to load its current contents and enter inline editing. Edit the optional name (up to 80 characters) and full multiline text. Empty registers are editable too.
- Click the green **Submit** button or elsewhere to save the edit. Clicking outside the menu also hides it after saving. Escape cancels changes. Opening an editor keeps the picker visible when Ctrl/Alt are released; standard Ctrl+C/V works inside the text fields.
- Clicking a history cell loads its text immediately. Application actions wait for an active edit to save first. Editing/using the picker keeps it open; use Hide/Esc or Ctrl+Alt+Space to return to the original app.
- Each populated/named register has a **trash** button that clears its name and contents only. History and the host clipboard remain unchanged. Register editing leaves the host clipboard at the value loaded when that register was clicked; the edited value is used on its next load.
- Drag history onto a register to save its **full text**, replacing the contents while preserving its name. The target highlights. A drop leaves the host clipboard unchanged unless the target is Current Clipboard.
- Previews show two lines. Full text preserves Unicode, whitespace, and remaining lines. An untouched editor does not rewrite its value or normalize its original line endings.
- **Save current** saves the host clipboard to a letter; **Clear all** empties every register name/content, history, and the host clipboard.
- The ring observes text every 120 ms, keeps 100 unique entries, and promotes saved/loaded entries. Repeated Paste adds no history.
- Entries are limited to 1 MiB. Register names, contents, and history stay in memory and disappear on exit. Clipboard text is never logged or persisted. Images, HTML, and files are not supported.
- Validation/write errors remain visible and preserve an active edit. Current clipboard writes wait for OS confirmation. Transient clipboard read failures retain the last known value; unsupported/absent text is shown explicitly.

## Start at login

Copy the release executable from `target\release\clipforge.exe` to `release\bin\x64\clipforge.exe` for distribution, then create a shortcut to that copy in your Windows Startup folder (`Win+R`, `shell:startup`). It runs silently on login. The cleanup script preserves the `release` folder. Launch directly from your desktop when testing; launches from an isolated automation desktop cannot observe your keyboard.

## Platforms

| Host | Status |
| --- | --- |
| Windows | Rust low-level hook, register prefixes and persistent Space menu, SendInput, clipboard sequence tracking, Tauri/WebView2 GUI. Local test results in TESTING.md. |
| macOS | Tauri system WebView; Ctrl+Alt+Space toggles the editor; C/V use a focused register chooser rather than global letter capture. Injection uses Command+C/V and needs Accessibility permission. Source not compiled or tested here. |
| Linux X11 | Tauri/WebKitGTK; chooser behavior as on macOS, Ctrl+C/V injection and selection timestamp tracking. Source not compiled or tested here. |
| Linux Wayland | Manual `--show` picker with Save current/Load; clipboard access needs compositor data-control support. Global shortcuts and automatic input unavailable. |

Windows cannot inject into elevated applications from a lower-integrity process. A VM may capture host shortcuts; guest tools must enable clipboard sharing. Copy waits up to three seconds for changed readable text; identical copies on X11 can need **Save current** when selection timestamps are unavailable.

Tauri native prerequisites are documented at https://v2.tauri.app/start/prerequisites/. Linux builds need GTK3, WebKitGTK 4.1, OpenSSL, librsvg, and X11 development libraries. CI includes those dependencies; macOS/Linux runtime validation still requires those hosts.

## Development and testing

```powershell
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
node tests/ui.test.cjs
node --check ui/app.js
cargo run --locked -- --smoke-ui
cargo run --locked -- --smoke-background
# Run opt-in real clipboard tests individually, not in parallel:
cargo test --test windows_smoke real_clipboard_register_ring_and_clear -- --ignored --test-threads=1
cargo test --test windows_smoke saving_history_text_to_register_preserves_exact_text_and_host_clipboard -- --ignored --test-threads=1
```

The desktop tests temporarily replace clipboard text, and the keyboard fixture changes focus. Only previous plain text is restored. See TESTING.md for validated checks and remaining limitations.

## Cleaning build output and caches

Compiled output stays in the ignored `target` directory inside the repository. Cargo's standard release executable path is `target/release/clipforge.exe`; distribution copies belong in the ignored `release/bin/x64` folder. Downloaded dependencies stay in Cargo's user-wide cache (normally `$HOME/.cargo`). Tauri may generate a small ignored `gen` directory; the cleanup script removes it. Keep `Cargo.lock` for reproducible dependency versions.

Development builds use line-table debug information and disable incremental compilation to reduce disk usage. Backtraces retain source locations; debugger variable inspection is limited, and recompiling edited code may take longer. These settings also apply to the inherited test profile; release settings are unchanged.

```powershell
# Remove Cargo build output:
cargo clean
# Preview removal of project build output, old distributions, and generated schemas:
.\scripts\clean.ps1 -WhatIf
# Remove those generated directories:
.\scripts\clean.ps1
# Also remove any project-local Cargo cache:
.\scripts\clean.ps1 -IncludeLocalCache
```

If Windows disables script execution, run it with a process-only policy override: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\clean.ps1 -IncludeLocalCache`. Add `-WhatIf` to preview. This does not change the system execution policy.

The script works from any directory and cleans generated directories inside this project. It preserves distribution copies in `release` and leaves the user-wide Cargo cache intact because other Rust projects share it. Before cleanup, copy release artifacts into `release/bin/x64`. The next build will recompile; deleting dependency caches also requires downloading them again. If you override `CARGO_TARGET_DIR`, use `cargo clean` to clean that location; the script only cleans the default project locations.

Cargo 1.88 and later automatically evict unused user-wide cache entries. This does not clean project `target` directories. You can configure how often eviction runs in your user Cargo configuration:

```toml
[cache]
auto-clean-frequency = "1 day"
```

Stable Cargo does not provide a command to clear all downloaded dependencies. Manual global cache cleanup and custom eviction ages currently require nightly Cargo; see the [Cargo cache cleanup documentation](https://doc.rust-lang.org/cargo/reference/unstable.html#gc). Do not delete the entire user `.cargo` directory: it also contains installed executables, configuration, and credentials.

`src/core.rs` owns storage, `src/input.rs` models Windows shortcuts, `src/platform` handles OS integration, `src/service.rs` serializes clipboard operations, and `src/picker.rs` owns selection state. `src/main.rs` hosts Tauri and exposes a small command bridge. `ui` contains the static frontend. Clipboard operations run off the UI thread; snapshots update the webview through Tauri events. The frontend renders clipboard contents with textContent and never interprets them as HTML.

