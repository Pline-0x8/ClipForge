# Development

Run commands from the repository root. For installation and usage, see [README.md](../README.md).

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
cargo test --locked --test windows_smoke clearing_ -- --ignored --test-threads=1
```

The desktop tests temporarily replace clipboard text, and the keyboard fixture changes focus. Only previous plain text is restored. See [TESTING.md](../TESTING.md) for validated checks and remaining limitations.

## Cleaning build output and caches

Compiled output stays in the ignored `target` directory inside the repository. Cargo's standard release executable path is `target/release/clipforge.exe`; distribution copies belong in the ignored `release/v<version>` folder (currently `release/v0.1.0-beta.1`). Keep only the executable and its `SHA256SUMS.txt` there. Release notes live in the tracked `docs/releases` directory. Downloaded dependencies stay in Cargo's user-wide cache (normally `$HOME/.cargo`). Tauri may generate a small ignored `gen` directory; the cleanup script removes it. Keep `Cargo.lock` for reproducible dependency versions.

Development builds use line-table debug information and disable incremental compilation to reduce disk usage. Backtraces retain source locations; debugger variable inspection is limited, and recompiling edited code may take longer. These settings also apply to the inherited test profile; release settings are unchanged.

```powershell
# Remove Cargo build output:
cargo clean
# Preview removal of project build output and generated schemas:
.\scripts\clean.ps1 -WhatIf
# Remove those generated directories:
.\scripts\clean.ps1
# Also remove any project-local Cargo cache:
.\scripts\clean.ps1 -IncludeLocalCache
```

If Windows disables script execution, run it with a process-only policy override: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\clean.ps1 -IncludeLocalCache`. Add `-WhatIf` to preview. This does not change the system execution policy.

The script works from any directory and cleans generated directories inside this project. It preserves distribution copies in `release` and leaves the user-wide Cargo cache intact because other Rust projects share it. Before cleanup, copy release artifacts into `release/v<version>` and generate a matching SHA-256 checksum. Avoid parallel distribution folders or temporary executable variants. The next build will recompile; deleting dependency caches also requires downloading them again. If you override `CARGO_TARGET_DIR`, use `cargo clean` to clean that location; the script only cleans the default project locations.

Cargo 1.88 and later automatically evict unused user-wide cache entries. This does not clean project `target` directories. You can configure how often eviction runs in your user Cargo configuration:

```toml
[cache]
auto-clean-frequency = "1 day"
```

Stable Cargo does not provide a command to clear all downloaded dependencies. Manual global cache cleanup and custom eviction ages currently require nightly Cargo; see the [Cargo cache cleanup documentation](https://doc.rust-lang.org/cargo/reference/unstable.html#gc). Do not delete the entire user `.cargo` directory: it also contains installed executables, configuration, and credentials.

`src/core.rs` owns storage, `src/input.rs` models Windows shortcuts, `src/platform` handles OS integration and transactional shortcut updates, `src/settings.rs` validates/persists hotkeys, `src/service.rs` serializes clipboard operations, and `src/picker.rs` owns selection state. `src/main.rs` hosts Tauri, the tray, and a small command bridge. `ui` contains the static frontend. Clipboard operations run off the UI thread; snapshots update the webview through Tauri events. The frontend renders clipboard contents with textContent and never interprets them as HTML.
