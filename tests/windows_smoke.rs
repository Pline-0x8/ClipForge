#![cfg(windows)]
//! Opt-in desktop integration test. Temporarily changes text clipboard and focus.
use clipforge::{
    platform::{self, Event},
    service::{self, Command, Update},
};
use std::{
    ptr::{null, null_mut},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Restore(Option<String>);
impl Drop for Restore {
    fn drop(&mut self) {
        if let Ok(mut c) = arboard::Clipboard::new()
            && let Some(s) = &self.0
        {
            let _ = c.set_text(s);
        }
    }
}
struct Window(windows_sys::Win32::Foundation::HWND, usize);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
        platform::focus(self.1);
    }
}
fn key(key: u16, up: bool) {
    let event = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    assert_eq!(
        unsafe { SendInput(1, &event, std::mem::size_of::<INPUT>() as i32) },
        1
    );
    pump();
}
fn prefix(key_code: u16) {
    key(VK_CONTROL, false);
    key(VK_MENU, false);
    key(key_code, false);
    key(key_code, true);
    key(VK_MENU, true);
    key(VK_CONTROL, true);
}
fn event(rx: &mpsc::Receiver<Event>) -> Event {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        pump();
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(10)) {
            return e;
        }
        assert!(Instant::now() < deadline, "Hook event missing");
    }
}
fn pump() {
    unsafe {
        let mut msg = std::mem::zeroed();
        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
fn snapshot(rx: &mpsc::Receiver<Update>, predicate: impl Fn(&clipforge::core::Engine) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump();
        if let Ok(update) = rx.recv_timeout(Duration::from_millis(10)) {
            match update {
                Update::Snapshot(e) if predicate(&e) => return,
                Update::Status(s) => eprintln!("status: {s}"),
                _ => {}
            }
        }
        assert!(
            Instant::now() < deadline,
            "Expected service snapshot never arrived"
        );
    }
}

#[test]
#[ignore = "Requires interactive desktop; temporarily changes clipboard and focus"]
fn real_windows_edit_copy_register_paste_history_and_clear() {
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let _restore = Restore(clipboard.get_text().ok());
    let old_focus = platform::target();
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("fixture selection 🌍").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | 0x0004,
            80,
            80,
            420,
            180,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    assert!(!hwnd.is_null(), "Could not create EDIT fixture");
    let _window = Window(hwnd, old_focus);
    unsafe {
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        SendMessageW(hwnd, 0x00b1, 0, -1);
    } // EM_SETSEL
    pump();
    clipboard.set_text("previous unrelated clipboard").unwrap();
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    let (events, event_rx) = mpsc::channel();
    platform::start(events).unwrap();
    prefix(b'C' as u16);
    key(b'X' as u16, false);
    key(b'X' as u16, true);
    let copied = event(&event_rx);
    assert!(matches!(copied, Event::Copy { register: 'x', .. }));
    tx.send(Command::Platform(copied)).unwrap();
    snapshot(&rx, |e| {
        e.registers()[23].as_deref() == Some("fixture selection 🌍")
    });
    // Copying identical text must still complete successfully on Windows.
    tx.send(Command::Platform(Event::Copy {
        register: 'y',
        target: hwnd as usize,
    }))
    .unwrap();
    snapshot(&rx, |e| {
        e.registers()[24].as_deref() == Some("fixture selection 🌍")
    });
    clipboard.set_text("ordinary copied text").unwrap();
    snapshot(&rx, |e| {
        e.history().first().map(String::as_str) == Some("ordinary copied text")
    });
    unsafe {
        SetWindowTextW(hwnd, wide("").as_ptr());
    }
    prefix(b'V' as u16);
    key(b'X' as u16, false);
    key(b'X' as u16, true);
    let pasted = event(&event_rx);
    assert!(matches!(pasted, Event::Paste { register: 'x', .. }));
    tx.send(Command::Platform(pasted)).unwrap();
    snapshot(&rx, |e| {
        e.history().first().map(String::as_str) == Some("fixture selection 🌍")
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        pump();
        let mut text = [0u16; 128];
        let len = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) };
        if String::from_utf16_lossy(&text[..len as usize]) == "fixture selection 🌍" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Native Ctrl+V failed to paste into EDIT"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(clipboard.get_text().unwrap(), "fixture selection 🌍");
    key(VK_CONTROL, false);
    key(VK_MENU, false);
    key(VK_SPACE, false);
    assert!(matches!(event(&event_rx), Event::Toggle { .. }));
    key(VK_SPACE, false);
    assert!(event_rx.recv_timeout(Duration::from_millis(50)).is_err());
    key(VK_SPACE, true);
    key(VK_MENU, true);
    key(VK_CONTROL, true);
    assert!(event_rx.recv_timeout(Duration::from_millis(100)).is_err());
    prefix(VK_SPACE);
    assert!(matches!(event(&event_rx), Event::Toggle { .. }));
    tx.send(Command::Clear).unwrap();
    snapshot(&rx, |e| {
        e.history().is_empty() && e.registers().iter().all(Option::is_none)
    });
    assert_eq!(clipboard.get_text().unwrap_or_default(), "");
    drop(tx);
    worker.join().unwrap();
}

#[test]
#[ignore = "Temporarily changes the real Windows text clipboard"]
fn real_clipboard_register_ring_and_clear() {
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let _restore = Restore(clipboard.get_text().ok());
    clipboard.set_text("ClipForge smoke: first 🌍").unwrap();
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    tx.send(Command::SaveCurrent('X')).unwrap();
    snapshot(&rx, |e| {
        e.registers()[23].as_deref() == Some("ClipForge smoke: first 🌍")
    });
    clipboard.set_text("ClipForge smoke: second").unwrap();
    snapshot(&rx, |e| {
        e.history().first().map(String::as_str) == Some("ClipForge smoke: second")
    });
    tx.send(Command::Load("ClipForge smoke: first 🌍".into()))
        .unwrap();
    snapshot(&rx, |e| {
        e.history().first().map(String::as_str) == Some("ClipForge smoke: first 🌍")
            && e.history().len() == 2
    });
    assert_eq!(clipboard.get_text().unwrap(), "ClipForge smoke: first 🌍");
    tx.send(Command::Clear).unwrap();
    snapshot(&rx, |e| {
        e.history().is_empty() && e.registers().iter().all(Option::is_none)
    });
    assert_eq!(clipboard.get_text().unwrap_or_default(), "");
    drop(tx);
    worker.join().unwrap();
}

#[test]
#[ignore = "Reads the real Windows clipboard; verifies errors never request a popup"]
fn empty_or_invalid_register_paste_reports_status_without_show() {
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    for register in ['q', '0'] {
        tx.send(Command::Platform(Event::Paste {
            register,
            target: 0,
        }))
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_status = false;
        loop {
            let update = rx.recv_timeout(Duration::from_millis(100));
            match update {
                Ok(Update::Show { .. }) => panic!("Register failure must not open the picker"),
                Ok(Update::Status(status)) => {
                    assert!(status.contains(if register == 'q' { "empty" } else { "Invalid" }));
                    saw_status = true;
                }
                Ok(Update::Snapshot(_)) if saw_status => break,
                _ => {}
            }
            assert!(
                Instant::now() < deadline,
                "Expected error status and snapshot"
            );
        }
    }
    // Also check the following monitor interval for delayed Show events.
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        assert!(!matches!(
            rx.recv_timeout(Duration::from_millis(30)),
            Ok(Update::Show { .. })
        ));
    }
    drop(tx);
    worker.join().unwrap();
}

#[test]
#[ignore = "Temporarily changes the real Windows text clipboard"]
fn saving_history_text_to_register_preserves_exact_text_and_host_clipboard() {
    let historical = "History 🌍\r\n世界\nthird line\twith spacing  ";
    let current = "Current clipboard must remain unchanged 🔒";
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let _restore = Restore(clipboard.get_text().ok());
    clipboard.set_text(historical).unwrap();
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    snapshot(&rx, |e| e.history().iter().any(|text| text == historical));
    clipboard.set_text(current).unwrap();
    snapshot(&rx, |e| {
        e.history().first().map(String::as_str) == Some(current)
    });
    tx.send(Command::SaveText {
        register: 'x',
        text: "old register text".into(),
    })
    .unwrap();
    snapshot(&rx, |e| {
        e.registers()[23].as_deref() == Some("old register text")
    });
    tx.send(Command::SaveText {
        register: 'X',
        text: historical.into(),
    })
    .unwrap();
    snapshot(&rx, |e| {
        e.registers()[23].as_deref() == Some(historical)
            && e.history()
                .iter()
                .filter(|text| text.as_str() == historical)
                .count()
                == 1
            && e.history().iter().any(|text| text == current)
            && e.history().iter().any(|text| text == "old register text")
    });
    assert_eq!(clipboard.get_text().unwrap(), current);
    drop(tx);
    worker.join().unwrap();
}

#[test]
#[ignore = "Temporarily changes the real Windows text clipboard"]
fn named_register_edit_clear_and_rejected_edits_preserve_host_clipboard() {
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let _restore = Restore(clipboard.get_text().ok());
    let current = "Host clipboard unchanged 🔒";
    let name = "VM 世界 🌍 ";
    let text = "first\r\n第二行\nthird\t  ";
    clipboard.set_text(current).unwrap();
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    tx.send(Command::EditRegister {
        register: 'X',
        name: name.into(),
        text: text.into(),
    })
    .unwrap();
    snapshot(&rx, |e| {
        e.register_names()[23] == name && e.registers()[23].as_deref() == Some(text)
    });
    tx.send(Command::EditRegister {
        register: 'a',
        name: "other".into(),
        text: "untouched".into(),
    })
    .unwrap();
    snapshot(&rx, |e| e.register_names()[0] == "other");
    tx.send(Command::SaveText {
        register: 'x',
        text: "replacement 🌍".into(),
    })
    .unwrap();
    snapshot(&rx, |e| {
        e.register_names()[23] == name && e.registers()[23].as_deref() == Some("replacement 🌍")
    });
    for (register, invalid_name, invalid_text) in [
        ('0', "invalid key".into(), "bad".into()),
        ('x', "🌍".repeat(81), "bad".into()),
        ('x', "valid name".into(), "x".repeat(1024 * 1024 + 1)),
    ] {
        tx.send(Command::EditRegister {
            register,
            name: invalid_name,
            text: invalid_text,
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_error = false;
        loop {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Update::Status(_)) => saw_error = true,
                Ok(Update::Snapshot(e)) if saw_error => {
                    assert_eq!(e.register_names()[23], name);
                    assert_eq!(e.registers()[23].as_deref(), Some("replacement 🌍"));
                    assert_eq!(e.register_names()[0], "other");
                    assert_eq!(e.registers()[0].as_deref(), Some("untouched"));
                    assert!(!e.history().iter().any(|s| s == "bad"));
                    break;
                }
                Ok(Update::Show { .. }) => panic!("Rejected edit must remain quiet"),
                _ => {}
            }
            assert!(
                Instant::now() < deadline,
                "Rejected edit did not report an error"
            );
        }
    }
    tx.send(Command::ClearRegister('X')).unwrap();
    snapshot(&rx, |e| {
        e.register_names()[23].is_empty()
            && e.registers()[23].is_none()
            && e.register_names()[0] == "other"
            && e.registers()[0].as_deref() == Some("untouched")
            && e.history().iter().any(|s| s == text)
            && e.history().iter().any(|s| s == "replacement 🌍")
    });
    assert_eq!(clipboard.get_text().unwrap(), current);
    drop(tx);
    worker.join().unwrap();
}

fn current_clipboard_update(rx: &mpsc::Receiver<Update>, expected: Option<&str>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(Update::Clipboard(text)) = rx.recv_timeout(Duration::from_millis(20))
            && text.as_deref() == expected
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Expected current clipboard update {expected:?}"
        );
    }
}

#[test]
#[ignore = "Temporarily changes the real Windows text clipboard"]
fn current_clipboard_updates_follow_external_and_loaded_text_but_not_register_edits() {
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let _restore = Restore(clipboard.get_text().ok());
    clipboard.set_text("initial current").unwrap();
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    current_clipboard_update(&rx, Some("initial current"));
    clipboard.set_text("external change 🌍\r\n世界").unwrap();
    current_clipboard_update(&rx, Some("external change 🌍\r\n世界"));
    let loaded = "loaded exact 🌍\r\n第二行\nthird\t  ";
    tx.send(Command::Load(loaded.into())).unwrap();
    current_clipboard_update(&rx, Some(loaded));
    let (reply, ack) = mpsc::channel();
    let edited = "acknowledged clipboard edit 🌍\nfull text";
    tx.send(Command::SetClipboard {
        text: edited.into(),
        reply,
    })
    .unwrap();
    assert_eq!(ack.recv_timeout(Duration::from_secs(2)).unwrap(), Ok(()));
    current_clipboard_update(&rx, Some(edited));
    let (reply, ack) = mpsc::channel();
    tx.send(Command::SetClipboard {
        text: "x".repeat(1024 * 1024 + 1),
        reply,
    })
    .unwrap();
    assert!(ack.recv_timeout(Duration::from_secs(2)).unwrap().is_err());
    tx.send(Command::EditRegister {
        register: 'X',
        name: "label".into(),
        text: "different register text".into(),
    })
    .unwrap();
    snapshot(&rx, |e| e.register_names()[23] == "label");
    tx.send(Command::ClearRegister('x')).unwrap();
    snapshot(&rx, |e| e.registers()[23].is_none());
    assert_eq!(clipboard.get_text().unwrap(), edited);
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        if let Ok(Update::Clipboard(text)) = rx.recv_timeout(Duration::from_millis(20)) {
            assert_eq!(
                text.as_deref(),
                Some(edited),
                "Register edits changed current clipboard"
            );
        }
    }
    clipboard.set_text("").unwrap();
    current_clipboard_update(&rx, Some(""));
    clipboard.clear().unwrap();
    current_clipboard_update(&rx, None);
    drop(tx);
    worker.join().unwrap();
}
