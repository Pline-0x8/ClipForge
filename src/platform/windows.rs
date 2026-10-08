use super::Event;
use crate::input::{Action, Input};
use std::{
    cell::RefCell,
    mem::size_of,
    ptr::null_mut,
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    System::{DataExchange::GetClipboardSequenceNumber, LibraryLoader::GetModuleHandleW},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const MARKER: usize = 0x434C495046524745;
const MENU_HOTKEY: i32 = 1;

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
                "ClipForge is already running. Press Ctrl+Alt+Space to open its picker.".into(),
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
    // Space belongs to RegisterHotKey. Swallowing it here prevents Windows
    // from generating WM_HOTKEY. Keep the hook exclusively for C/V prefixes.
    if data.vkCode == u32::from(VK_SPACE) {
        if down {
            STATE.with(|s| {
                if let Some(s) = s.borrow_mut().as_mut() {
                    s.input.cancel_prefix();
                }
            });
        }
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
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
        let decision = state.input.handle(
            data.vkCode as u16,
            down,
            ctrl,
            alt,
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
pub fn start_backend(tx: Sender<Event>) -> Result<(), String> {
    start_backend_thread(tx).map(|_| ())
}
fn start_backend_thread(tx: Sender<Event>) -> Result<u32, String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::spawn(move || unsafe {
        let mut keys = [false; 256];
        for key in [VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU] {
            keys[key as usize] = GetAsyncKeyState(key as i32) < 0;
        }
        STATE.with(|s| {
            *s.borrow_mut() = Some(HookState {
                input: Input::default(),
                tx,
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
        if RegisterHotKey(
            null_mut(),
            MENU_HOTKEY,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            u32::from(VK_SPACE),
        ) == 0
        {
            let code = windows_sys::Win32::Foundation::GetLastError();
            UnhookWindowsHookEx(handle);
            let _ = ready_tx.send(Err(format!("Could not register Ctrl+Alt+Space (Windows error {code}). Another application may own this shortcut.")));
            return;
        }

        let _ = ready_tx.send(Ok(
            windows_sys::Win32::System::Threading::GetCurrentThreadId(),
        ));
        loop {
            let mut msg = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    UnregisterHotKey(null_mut(), MENU_HOTKEY);
                    UnhookWindowsHookEx(handle);
                    return;
                }
                if msg.message == WM_HOTKEY && msg.wParam == MENU_HOTKEY as usize {
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
        let thread_id = start_backend_thread(tx).expect("Native shortcut backend must start");
        // Exercise the real Win32 queue and backend dispatch, without synthesizing
        // physical keys or changing clipboard/focus on the user's desktop.
        unsafe {
            assert_eq!(
                RegisterHotKey(
                    null_mut(),
                    99,
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    u32::from(VK_SPACE)
                ),
                0,
                "Backend must actually own Ctrl+Alt+Space"
            );
            assert_ne!(
                PostThreadMessageW(thread_id, WM_HOTKEY, MENU_HOTKEY as usize, 0),
                0
            );
        }
        let received = rx.recv_timeout(Duration::from_secs(2));
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
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    u32::from(VK_SPACE)
                ),
                0,
                "Backend shutdown must release the hotkey"
            );
            assert_ne!(UnregisterHotKey(null_mut(), 99), 0);
        }
    }
}
