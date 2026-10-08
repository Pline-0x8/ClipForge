<p align="center">
  <img src="docs/images/clipforge.png" width="128" height="128" alt="ClipForge: a mint clipboard and forge spark">
</p>

<h1 align="center">ClipForge</h1>
<p align="center">Keep useful text in letter registers. Bring your clipboard history back when you need it.</p>
<p align="center">
  <a href="https://github.com/Pline-0x8/ClipForge/releases">Downloads</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#keyboard-shortcuts">Shortcuts</a> ·
  <a href="docs/DEVELOPMENT.md">Development</a>
</p>

ClipForge is a desktop clipboard manager built with Rust and Tauri. It runs quietly in your system tray, gives you **26 named registers (A–Z)** for reusable text, and keeps **100 unique recent clipboard entries**. Save a command, a quick reply, or a snippet to a letter and paste it again with a shortcut.

![ClipForge picker showing the current clipboard, named letter registers, and recent text history](docs/images/picker.png)

*The actual frontend, captured in Chrome with sample data. These documentation screenshots do not access the host clipboard.*

## Features

- **Keep snippets on a letter.** Name registers and store full multiline text.
- **Recover recent copies.** History removes duplicates and moves reused text to the top.
- **Edit in place.** Click the current clipboard or a register to edit its text.
- **Drag to save.** Drop history or current clipboard text onto a register; drop history onto Current clipboard to load it.
- **Choose your shortcuts.** Configure picker, register-copy, and register-paste hotkeys from the gear button.
- **Keep clipboard data in memory.** Registers, names, and history disappear on exit. Only hotkey settings persist.

## Install on Windows

1. Open [Releases](https://github.com/Pline-0x8/ClipForge/releases) and download `clipforge.exe` from a release that includes a Windows asset.
2. Put it in a folder where you want to keep it, then run it. No installer is required.
3. Click the ClipForge tray icon, choose **Open ClipForge** from its menu, or press **Ctrl+Alt+Space**.

Windows needs the **Microsoft WebView2 Runtime**. If a release executable is not available, [build from source](#build-from-source).

To start at login, create a shortcut to the executable in your Windows Startup folder (`Win+R`, then `shell:startup`). ClipForge starts hidden with no taskbar button.

## Quick start

### Save a snippet and paste it later

On Windows, using the default shortcuts:

1. Select text in another application.
2. Press **Ctrl+Alt+C**, release **C**, then press **X** within two seconds. ClipForge copies the selection into register X.
3. Move to the application where you want the text.
4. Press **Ctrl+Alt+V**, release **V**, then press **X**. Release the modifiers so ClipForge can send native Paste.

Letters are case-insensitive. You can release Ctrl and Alt before pressing the register letter. Ordinary Ctrl+C and Ctrl+V continue to work normally.

For applications that copy with a different shortcut, copy normally, open ClipForge, click **Save current…**, and choose a letter.

### Use history and drag/drop

Open the picker with **Ctrl+Alt+Space**. Current clipboard is at the top, registers are on the left, and history is on the right.

- Click a history entry to load its full text into the host clipboard. Hide the picker and paste normally.
- Drag history onto a register to save the full text, preserving its name and leaving the host clipboard unchanged.
- Drag history onto **Current clipboard** to load it, or drag Current clipboard onto a register to save it.
- Use arrows or Tab to highlight a populated entry. Enter loads it and closes the picker.

Previews show two lines; stored text preserves the remaining lines, Unicode, and whitespace.

### Name and edit a register

Click a register to open its inline editor. A populated register also loads its existing text into the current clipboard when clicked. Add a name, edit the full text, and click **Submit** or elsewhere to save. **Escape** cancels the edit.

![Inline register editor showing a Development VM name, multiline SSH command, and Submit button](docs/images/register-edit.png)

*Editing register X in the same demo frontend. The edited text is used on its next load; saving the register does not rewrite the host clipboard.*

Empty registers are editable too. Names can contain up to 80 characters. Untouched editors preserve original line endings. Validation or write errors keep the edit available for correction or retry.

The trash button clears one register's name and contents, leaving history and the host clipboard unchanged. **Clear all** empties every register, history, and the host clipboard.

## Keyboard shortcuts

| Default keys | Action |
| --- | --- |
| Ctrl+Alt+Space | Open or hide the picker |
| Ctrl+Alt+C, release C, then A–Z | Copy selected text into a register on Windows |
| Ctrl+Alt+V, release V, then A–Z | Load a register and paste it on Windows |
| A–Z in the picker | Load and paste that register when no editor or dialog is active |
| Arrows / Tab / Shift+Tab | Navigate populated registers and history |
| Enter | Load the highlighted entry and close the picker |
| Escape | Cancel an edit, or close the picker |
| Ctrl+C / Ctrl+V | Normal copy/paste inside editors and other applications |

The copy/paste prefix expires after two seconds. Holding its activation key never opens the menu. The picker stays open when modifiers are released. Clicking outside or toggling it saves an active edit before hiding; errors keep the edit available. **Hide** closes the picker; **Quit** exits the application.

### Change the hotkeys

Click the **gear**. Choose modifier checkboxes and one key for each action: Space, a letter, a digit, or F1–F24. Include Ctrl, Alt, or Win / Command, and give each action a distinct shortcut.

**Save** applies and persists shortcuts immediately. Conflicting or reserved shortcuts show an error and retain the previous configuration. **Restore defaults** fills the defaults; Save applies them. Cancel or Escape discards changes. Clicking outside or toggling the picker saves changed settings before hiding and retains the dialog on errors.

On Windows, settings live in `%APPDATA%\dev.clipforge.desktop\hotkeys.json`. Invalid saved settings produce an error and use defaults for that session. Global hotkey settings are unavailable on Wayland.

## Data and platform behavior

ClipForge supports **plain text only**, up to **1 MiB per entry**. Images, HTML clipboard formats, and files are not supported. The service observes clipboard text every 120 ms and keeps 100 unique nonempty history entries. Repeated Paste does not add duplicates.

Clipboard contents, register names, and history are never logged or persisted by ClipForge. They disappear on exit. The host clipboard may still contain the last loaded text after ClipForge quits.

| Platform | Behavior |
| --- | --- |
| Windows | Global letter prefixes, native Copy/Paste, and a Tauri/WebView2 picker. Local validation is recorded in [TESTING.md](TESTING.md). |
| macOS | System WebView and a focused register chooser for copy/paste shortcuts. Injection uses Command+C/V and requires Accessibility permission. Runtime behavior has not been validated here. |
| Linux X11 | Focused register chooser, Ctrl+C/V injection, and selection timestamp tracking. Runtime behavior has not been validated here. |
| Linux Wayland | Manual `--show` picker with Save current and clipboard loading. Clipboard access needs compositor data-control support; global shortcuts and automatic input are unavailable. |

Windows cannot inject into elevated applications from a lower-integrity process. A VM may capture host shortcuts; enable clipboard sharing in VMware or your guest tools. ClipForge uses the host clipboard; guest tools handle VM sharing.

Copy waits up to three seconds for new readable text. Identical copies on X11 can require **Save current…** when selection timestamps are unavailable. Transient read failures retain the last known clipboard value; absent or unsupported text is shown explicitly.

## Build from source

Windows needs a recent stable Rust toolchain, MSVC build tools, and Microsoft WebView2 Runtime. See [Tauri's native prerequisites](https://v2.tauri.app/start/prerequisites/) for macOS and Linux requirements.

```powershell
git clone https://github.com/Pline-0x8/ClipForge.git
cd ClipForge
cargo build --release --locked
.\target\release\clipforge.exe
```

To open the picker immediately:

```powershell
.\target\release\clipforge.exe --show
```

The frontend is static HTML/CSS/JavaScript. No npm install or frontend build is required. Node is needed only for frontend tests and the optional screenshot script. Installer bundling is currently disabled.

## Development

```powershell
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
node tests/ui.test.cjs
node --check ui/app.js
```

CI tests and builds on Windows, macOS, and Linux. Native desktop validation is separate from unit tests. See [TESTING.md](TESTING.md) for smoke checks, opt-in clipboard tests, and validation gaps. [Development](docs/DEVELOPMENT.md) preserves the build-output and cleanup instructions.

| Location | Responsibility |
| --- | --- |
| `src/core.rs` | In-memory registers and history |
| `src/service.rs` | Clipboard operations on a worker thread |
| `src/input.rs` | Windows shortcut state machine |
| `src/platform/` | Native shortcuts, focus, input injection, and hotkey registration |
| `src/settings.rs` | Hotkey validation and persistence |
| `src/picker.rs` | Selection state and snapshots |
| `src/main.rs` | Tauri runtime, tray, and command bridge |
| `ui/` | Static frontend |

Snapshots reach the frontend through Tauri events. Clipboard operations run off the UI thread; clipboard text is rendered with `textContent`.

### Artwork and screenshots

The generated master is [`icons/clipforge-source.png`](icons/clipforge-source.png). The tray uses `icons/icon.png`; the Windows executable uses `icons/icon.ico`. Regenerate the PNG sizes and multi-resolution ICO on Windows with:

```powershell
.\scripts\build-icons.ps1
```

Regenerate the demo screenshots with Node 22 or later and Google Chrome:

```powershell
node scripts/capture-docs.cjs
```

Set `CLIPFORGE_CHROME` to Chrome's executable path if it differs from the Windows default. The script renders the actual frontend through a demo IPC bridge in a separate temporary profile, without monitoring the clipboard or registering shortcuts.

## License

[MIT](LICENSE)
