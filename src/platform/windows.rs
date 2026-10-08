use super::Event;
use crate::input::{Action, Input};
use crate::settings::{self, Hotkeys, Shortcut};
use std::{
    cell::RefCell,
    mem::size_of,
    path::PathBuf,
    ptr::null_mut,
    sync::{
        OnceLock,
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    System::{DataExchange::GetClipboardSequenceNumber, LibraryLoader::GetModuleHandleW},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const MARKER: usize = 0x434C495046524745;
struct Configure {
    hotkeys: Hotkeys,
    path: PathBuf,
    reply: Sender<Result<(), String>>,
}
static CONFIGURE: OnceLock<Sender<Configure>> = OnceLock::new();

pub fn configure(hotkeys: &Hotkeys, path: &std::path::Path) -> Result<(), String> {
    let (reply, result) = mpsc::channel();
    CONFIGURE
        .get()
        .ok_or("Shortcut backend is unavailable")?
        .send(Configure {
            hotkeys: hotkeys.clone(),
            path: path.to_owned(),
            reply,
        })
        .map_err(|_| "Shortcut backend is unavailable")?;
    result
        .recv()
        .map_err(|_| "Shortcut backend is unavailable")?
}

pub struct Instance(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub fn acquire_instance() -> Result<Instance, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateMutexW,
    };
    let name: Vec<u16> = "Local\\ClipForge-Desktop"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            return Err("Could not acquire ClipForge instance lock".into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return Err(
                "ClipForge is already running. Use its tray icon or configured menu shortcut to open the picker.".into(),
            );
        }
        Ok(Instance(handle))
    }
}

pub fn startup_message(message: &str) {
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "ClipForge".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
struct HookState {
    input: Input,
    tx: Sender<Event>,
    keys: [bool; 256],
}
thread_local! { static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) }; }
fn dispatch(tx: &Sender<Event>, action: Action) {
    let event = match action {
        Action::Copy { register, target } => Event::Copy { register, target },
        Action::Paste { register, target } => Event::Paste { register, target },
        Action::Toggle { target } => Event::Toggle { target },
    };
    let _ = tx.send(event);
}
unsafe extern "system" fn hook(code: i32, wparam: usize, lparam: isize) -> isize {
    if code < 0 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let data = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    if data.dwExtraInfo == MARKER || data.vkCode >= 256 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
    let suppress = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return false;
        };
        let tracked_key = match data.vkCode as u16 {
            VK_CONTROL if data.flags & LLKHF_EXTENDED != 0 => VK_RCONTROL,
            VK_CONTROL => VK_LCONTROL,
            VK_MENU if data.flags & LLKHF_EXTENDED != 0 => VK_RMENU,
            VK_MENU => VK_LMENU,
            key => key,
        };
        state.keys[tracked_key as usize] = down;
        let ctrl = [VK_LCONTROL, VK_RCONTROL]
            .iter()
            .any(|&k| state.keys[k as usize]);
        let alt = [VK_LMENU, VK_RMENU].iter().any(|&k| state.keys[k as usize]);
        let shift = [VK_LSHIFT, VK_RSHIFT]
            .iter()
            .any(|&k| state.keys[k as usize]);
        let win = [VK_LWIN, VK_RWIN].iter().any(|&k| state.keys[k as usize]);
        let modifiers = (u32::from(ctrl) * settings::CTRL)
            | (u32::from(alt) * settings::ALT)
            | (u32::from(shift) * settings::SHIFT)
            | (u32::from(win) * settings::SUPER);
        // Let Windows generate WM_HOTKEY for the menu. Prefixes are consumed
        // by the hook; their native registrations reserve them for conflict detection.
        if state.input.menu_matches(data.vkCode as u16, modifiers)
            && !state.input.is_swallowed(data.vkCode as u16)
        {
            if down {
                state.input.cancel_prefix();
            }
            return false;
        }
        let decision = state.input.handle_modifiers(
            data.vkCode as u16,
            down,
            modifiers,
            Instant::now(),
            target(),
        );
        if let Some(action) = decision.action {
            dispatch(&state.tx, action);
        }
        decision.suppress
    });
    if suppress {
        1
    } else {
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }
}
pub fn start_backend(tx: Sender<Event>, hotkeys: &Hotkeys) -> Result<(), String> {
    let (sender, rx) = mpsc::channel();
    let _ = CONFIGURE.set(sender);
    start_backend_thread(tx, hotkeys.clone(), rx).map(|_| ())
}
// Register additions before retiring any old registration. Matching shortcuts
// retain their IDs, including when actions exchange shortcuts.
fn stage(
    shortcuts: [Shortcut; 3],
    active: &[(Shortcut, i32)],
    next_id: &mut i32,
) -> Result<Vec<(Shortcut, i32)>, String> {
    let mut staged = Vec::new();
    for (index, shortcut) in shortcuts.into_iter().enumerate() {
        if let Some(existing) = active.iter().find(|(key, _)| *key == shortcut) {
            staged.push(*existing);
            continue;
        }
        *next_id += 1;
        let id = *next_id;
        if unsafe {
            RegisterHotKey(
                null_mut(),
                id,
                shortcut.modifiers | MOD_NOREPEAT,
                u32::from(shortcut.key),
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            retire(&staged, active);
            return Err(format!(
                "{} shortcut {} is unavailable: {error}. Another application or Windows may own it.",
                ["Menu", "Register copy", "Register paste"][index],
                shortcut.label()
            ));
        }
        staged.push((shortcut, id));
    }
    Ok(staged)
}
fn retire(old: &[(Shortcut, i32)], keep: &[(Shortcut, i32)]) {
    for (_, id) in old {
        if !keep.iter().any(|(_, kept)| id == kept) {
            unsafe {
                UnregisterHotKey(null_mut(), *id);
            }
        }
    }
}
fn start_backend_thread(
    tx: Sender<Event>,
    hotkeys: Hotkeys,
    requests: mpsc::Receiver<Configure>,
) -> Result<u32, String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::spawn(move || unsafe {
        let mut keys = [false; 256];
        for key in [
            VK_LCONTROL,
            VK_RCONTROL,
            VK_LMENU,
            VK_RMENU,
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LWIN,
            VK_RWIN,
        ] {
            keys[key as usize] = GetAsyncKeyState(key as i32) < 0;
        }
        STATE.with(|s| {
            *s.borrow_mut() = Some(HookState {
                input: Input::default(),
                tx: tx.clone(),
                keys,
            })
        });
        let handle = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if handle.is_null() {
            let _ = ready_tx.send(Err("Could not install keyboard hook".to_owned()));
            return;
        }
        let mut next_id = 0;
        let initial = hotkeys
            .parsed()
            .and_then(|keys| stage(keys, &[], &mut next_id));
        let mut active = match initial {
            Ok(active) => {
                STATE.with(|s| {
                    s.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .input
                        .configure(hotkeys.parsed().unwrap())
                });
                let _ = ready_tx.send(Ok(
                    windows_sys::Win32::System::Threading::GetCurrentThreadId(),
                ));
                active
            }
            Err(error) => {
                // Keep the thread alive so the settings dialog can recover.
                STATE.with(|s| *s.borrow_mut() = None);
                let _ = ready_tx.send(Err(error));
                Vec::new()
            }
        };
        // Preserve an event sender even when initial registration fails.
        let event_tx = tx;
        loop {
            while let Ok(request) = requests.try_recv() {
                let result = request.hotkeys.parsed().and_then(|shortcuts| {
                    let staged = stage(shortcuts, &active, &mut next_id)?;
                    if let Err(error) = settings::save(&request.path, &request.hotkeys) {
                        retire(&staged, &active);
                        return Err(error);
                    }
                    retire(&active, &staged);
                    active = staged;
                    STATE.with(|s| {
                        let mut state = s.borrow_mut();
                        let state = state.get_or_insert_with(|| HookState {
                            input: Input::default(),
                            tx: event_tx.clone(),
                            keys: [false; 256],
                        });
                        state.input.configure(shortcuts);
                    });
                    Ok(())
                });
                let _ = request.reply.send(result);
            }
            let mut msg = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    retire(&active, &[]);
                    UnhookWindowsHookEx(handle);
                    return;
                }
                if msg.message == WM_HOTKEY
                    && active
                        .first()
                        .is_some_and(|(_, id)| msg.wParam == *id as usize)
                {
                    STATE.with(|s| {
                        if let Some(s) = s.borrow_mut().as_mut() {
                            s.input.cancel_prefix();
                            let _ = s.tx.send(Event::Toggle { target: target() });
                        }
                    });
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut()
                    && let Some(action) = s.input.tick(Instant::now())
                {
                    dispatch(&s.tx, action);
                }
            });
            thread::sleep(Duration::from_millis(8));
        }
    });
    ready_rx.recv().map_err(|e| e.to_string())?
}
pub fn sequence() -> u64 {
    unsafe { GetClipboardSequenceNumber() as u64 }
}
pub fn target() -> usize {
    unsafe { GetForegroundWindow() as usize }
}
pub fn focus(target: usize) {
    if target != 0 {
        unsafe {
            SetForegroundWindow(target as _);
        }
    }
}

pub fn reveal_picker(window: usize, original: usize) {
    if window != 0 {
        unsafe {
            use windows_sys::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            };
            let monitor = MonitorFromWindow(original as _, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            let mut rect = std::mem::zeroed();
            if GetMonitorInfoW(monitor, &mut info) != 0
                && GetWindowRect(window as _, &mut rect) != 0
            {
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                SetWindowPos(
                    window as _,
                    HWND_TOPMOST,
                    info.rcWork.left + (info.rcWork.right - info.rcWork.left - width) / 2,
                    info.rcWork.top + (info.rcWork.bottom - info.rcWork.top - height) / 2,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            ShowWindow(window as _, SW_SHOW);
            SetForegroundWindow(window as _);
        }
    }
}

pub fn picker_visible(window: usize) -> bool {
    window != 0 && unsafe { IsWindowVisible(window as _) != 0 }
}
pub fn native_shortcut(copy: bool, destination: usize) -> Result<u64, String> {
    if copy && target() != destination {
        return Err("Application focus changed before Copy; try again".into());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let modifiers_down = unsafe {
            [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
                .iter()
                .any(|&k| GetAsyncKeyState(k as i32) < 0)
        };
        if !modifiers_down {
            break;
        }
        if Instant::now() >= deadline {
            return Err("Release modifier keys and try again".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    focus(destination);
    if target() != destination {
        return Err("Original application could not regain focus".into());
    }
    let key = if copy { b'C' } else { b'V' } as u16;
    let input = |key, up| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: MARKER,
            },
        },
    };
    let inputs = [
        input(VK_CONTROL, false),
        input(key, false),
        input(key, true),
        input(VK_CONTROL, true),
    ];
    let baseline = sequence();
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        if sent > 0 {
            // Release any partially inserted shortcut, avoiding stuck modifiers.
            let releases = [input(key, true), input(VK_CONTROL, true)];
            unsafe {
                SendInput(2, releases.as_ptr(), size_of::<INPUT>() as i32);
            }
        }
        Err("Input injection failed (an elevated application may reject it)".into())
    } else {
        Ok(baseline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_hotkey_registration_dispatch_and_cleanup() {
        let (tx, rx) = mpsc::channel();
        let (configure_tx, requests) = mpsc::channel();
        let keys = Hotkeys {
            menu: "Ctrl+Alt+Shift+F21".into(),
            copy: "Ctrl+Alt+Shift+F22".into(),
            paste: "Ctrl+Alt+Shift+F23".into(),
        };
        let thread_id = start_backend_thread(tx, keys.clone(), requests)
            .expect("Native shortcut backend must start");
        // Exercise the real Win32 queue and backend dispatch, without synthesizing
        // physical keys or changing clipboard/focus on the user's desktop.
        unsafe {
            assert_eq!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    0x84
                ),
                0,
                "Backend must actually own the configured menu shortcut"
            );
            assert_ne!(PostThreadMessageW(thread_id, WM_HOTKEY, 1, 0), 0);
        }
        let received = rx.recv_timeout(Duration::from_secs(2));
        let path = std::env::temp_dir()
            .join(format!("clipforge-native-{}", std::process::id()))
            .join("hotkeys.json");
        unsafe {
            assert_ne!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    0x87
                ),
                0
            );
        }
        let conflicting = Hotkeys {
            menu: "Ctrl+Alt+Shift+F20".into(),
            paste: "Ctrl+Alt+Shift+F24".into(),
            ..keys.clone()
        };
        let (reply, result) = mpsc::channel();
        configure_tx
            .send(Configure {
                hotkeys: conflicting,
                path: path.clone(),
                reply,
            })
            .unwrap();
        assert!(
            result
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_err()
        );
        assert!(!path.exists(), "Conflicts must not persist new settings");
        unsafe {
            UnregisterHotKey(null_mut(), 99);
            assert_ne!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    0x83
                ),
                0,
                "Conflict releases successfully staged additions"
            );
            UnregisterHotKey(null_mut(), 99);
            assert_eq!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    0x84
                ),
                0,
                "Conflict retains the original menu shortcut"
            );
        }
        let swapped = Hotkeys {
            menu: keys.copy.clone(),
            copy: keys.menu.clone(),
            paste: keys.paste.clone(),
        };
        let (reply, result) = mpsc::channel();
        configure_tx
            .send(Configure {
                hotkeys: swapped.clone(),
                path: path.clone(),
                reply,
            })
            .unwrap();
        assert!(result.recv_timeout(Duration::from_secs(2)).unwrap().is_ok());
        assert_eq!(settings::load(&path).unwrap(), swapped);
        let bad_path = path.join("hotkeys.json"); // A file cannot be a parent directory.
        let changed = Hotkeys {
            menu: "Ctrl+Alt+Shift+F24".into(),
            ..keys.clone()
        };
        let (reply, result) = mpsc::channel();
        configure_tx
            .send(Configure {
                hotkeys: changed,
                path: bad_path,
                reply,
            })
            .unwrap();
        assert!(
            result
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap_err()
                .contains("save")
        );
        assert_eq!(
            settings::load(&path).unwrap(),
            swapped,
            "Write failure preserves stored settings"
        );
        unsafe {
            assert_ne!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    0x87
                ),
                0,
                "Write failure releases staged registration"
            );
            UnregisterHotKey(null_mut(), 99);
        }
        unsafe {
            PostThreadMessageW(thread_id, WM_HOTKEY, 2, 0);
        }
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(2)),
            Ok(Event::Toggle { .. })
        ));
        // Always release the native registration, even if the assertion fails.
        unsafe {
            PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
        }
        assert!(
            matches!(received, Ok(Event::Toggle { .. })),
            "Actual backend must deliver a toggle: {received:?}"
        );
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        unsafe {
            assert_ne!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    0x84
                ),
                0,
                "Backend shutdown must release the hotkey"
            );
            assert_ne!(UnregisterHotKey(null_mut(), 99), 0);
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
