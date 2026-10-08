# ClipForge testing

Run the automated checks:

```powershell
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
node tests/ui.test.cjs
node --check ui/app.js
cargo build --locked --release
```

CI runs tests, Clippy, and release builds on Windows, macOS, and Linux. Rust tests cover storage, history, shortcut handling, picker state, clipboard read failures, and Windows hotkey registration. Frontend tests run the actual app against a fake DOM and IPC bridge; they do not validate a native WebView.

## Interactive desktop validation

```powershell
cargo run --locked -- --smoke-ui
cargo run --locked -- --smoke-background
```

Smoke checks verify tray/menu creation and retention, WebView initialization, and frontend-to-Rust communication. They use sample data without clipboard monitoring or hooks, have a 20-second watchdog, and store temporary profiles in `webview-smoke` beside the executable (normally `target/debug/webview-smoke`). They may briefly show a separate tray icon while leaving any running instance untouched. The background check also verifies hidden startup and error status.

Opt-in Windows integration tests temporarily replace clipboard text; the keyboard fixture also changes focus. Only previous plain text is restored. Run tests individually from an interactive terminal, with one test thread:

```powershell
cargo test --locked --test windows_smoke real_clipboard_register_ring_and_clear -- --ignored --test-threads=1
cargo test --locked --test windows_smoke saving_history_text_to_register_preserves_exact_text_and_host_clipboard -- --ignored --test-threads=1
cargo test --locked --test windows_smoke clearing_ -- --ignored --test-threads=1
cargo test --locked --test rich_windows -- --ignored --test-threads=1
```

Previous Windows validation recorded passing Rust unit tests, frontend tests, real clipboard integration tests, Clippy, and an optimized executable build. Live WebView rendering and native keyboard injection were not verified in the restricted test environment. macOS/Linux runtime behavior and VMware sharing remain unverified.

Rich-history tests round-trip PNG image bytes, Unicode file lists, tab-separated cells with an additional registered workbook format, and a custom binary format through the real Windows clipboard. They verify original captured format bytes, typed previews, register preservation, history clearing, and expired-entry errors. This fixture test restores previously captured memory-backed formats. Existing `windows_smoke` tests restore plain text only. The tests do not establish compatibility with every Excel version or process-specific OLE/virtual file format. PDF rendering is not implemented.

The mixed-history update also passes all seven clipboard/status integration tests without keyboard injection (`cargo test --locked --test windows_smoke -- --ignored --test-threads=1 --skip real_windows_edit_copy_register_paste_history_and_clear`). Native capture and text reads/writes now share a serialized, non-null clipboard owner; regression checks cover rapid clipboard changes alongside the monitor.

The October 8, 2026 tray/settings update passed 23 Rust tests, Clippy with warnings denied, frontend tests, JavaScript syntax checks, and both desktop smoke checks. Native tests use unusual Ctrl+Alt+Shift+F21–F24 shortcuts so they can run alongside an instance using defaults. They verify real registration, dispatch, action swaps, conflict rollback, persistence failure rollback, and cleanup without injecting keys or changing the clipboard. Frontend tests cover settings save/cancel/defaults, autosave before opening settings, pending-save controls, error retention on blur, toggle-to-save, and tray Quit autosave. Smoke processes reported a WebView2 window-class cleanup warning after successful initialization checks. Manual tray-click behavior, settings layout, and physical custom hotkey use still need interactive desktop validation; macOS/Linux builds were not run here.

Before release, check hidden startup, menu toggling, copy/paste prefixes, focus restoration, inline edits, cancellation, validation, drag/drop, and Quit on an interactive desktop. Verify VM clipboard sharing if used. Windows cannot inject into elevated applications from a lower-integrity process; macOS requires Accessibility permission; Wayland supports manual clipboard actions only.
