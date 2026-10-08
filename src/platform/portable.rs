//! macOS and X11 use a focused register chooser rather than swallowing arbitrary
//! global letter keys. Wayland deliberately exposes only the manual chooser.
use super::Event;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use std::sync::mpsc::Sender;

pub fn start_backend(tx: Sender<Event>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Err("Wayland: global register shortcuts are unavailable; use the picker to save/load clipboard text, then paste normally.".into());
    }
    // On macOS this MUST run on the UI/main thread, where Tauri pumps events.
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let copy = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyC);
    let paste = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);
    let menu = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    manager.register(menu).map_err(|e| e.to_string())?;
    manager.register(copy).map_err(|e| e.to_string())?;
    manager.register(paste).map_err(|e| e.to_string())?;
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        // Release avoids injecting native Copy while the activation key is held.
        if event.state == HotKeyState::Released {
            if event.id == menu.id() {
                let _ = tx.send(Event::Toggle { target: target() });
            } else if event.id == copy.id() {
                let _ = tx.send(Event::PrepareCopy { target: target() });
            } else if event.id == paste.id() {
                let _ = tx.send(Event::Show {
                    copy: false,
                    target: target(),
                });
            }
        }
    }));
    // One manager per process, retained until process exit to retain registration.
    Box::leak(Box::new(manager));
    Ok(())
}

pub fn sequence() -> u64 {
    #[cfg(target_os = "macos")]
    {
        mac::sequence()
    }
    #[cfg(target_os = "linux")]
    {
        x11::sequence()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

pub fn target() -> usize {
    #[cfg(target_os = "macos")]
    {
        mac::target()
    }
    #[cfg(target_os = "linux")]
    {
        x11::target()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

pub fn focus(target: usize) {
    if target == 0 {
        return;
    }
    #[cfg(target_os = "macos")]
    mac::focus(target);
    #[cfg(target_os = "linux")]
    x11::focus(target);
}

pub fn native_shortcut(copy: bool, target: usize) -> Result<u64, String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Err(
            "Wayland automatic Copy/Paste is unavailable; use the application's native shortcut."
                .into(),
        );
    }
    if target == 0 {
        return Err("Could not identify the original application; use native Copy/Paste.".into());
    }
    focus(target);
    // Give the desktop time to honor activation before injecting.
    std::thread::sleep(std::time::Duration::from_millis(150));
    if self::target() != target {
        return Err("The original application could not be focused; no shortcut was sent.".into());
    }
    // Never release somebody else's physically held modifiers synthetically.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        #[cfg(target_os = "linux")]
        let released = x11::modifiers_released()?;
        #[cfg(target_os = "macos")]
        let released = mac::modifiers_released();
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let released = false;
        if released {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("Release Ctrl, Alt, Shift and Command before copying/pasting.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if self::target() != target {
        return Err("Application focus changed while waiting; no shortcut was sent.".into());
    }
    let mut input = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;
    let baseline = sequence();
    if self::target() != target {
        return Err(
            "Application focus changed while reading the clipboard; no shortcut was sent.".into(),
        );
    }
    input
        .key(modifier, Direction::Press)
        .map_err(|e| e.to_string())?;
    let result = input.key(Key::Unicode(if copy { 'c' } else { 'v' }), Direction::Click);
    // Release even on an injection error, avoiding a stuck synthetic modifier.
    let release = input.key(modifier, Direction::Release);
    result
        .and(release)
        .map(|()| baseline)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
mod x11 {
    use x11rb::{
        connection::Connection,
        protocol::xproto::{self, ConnectionExt},
    };
    pub fn modifiers_released() -> Result<bool, String> {
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let mapping = conn
            .get_modifier_mapping()
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let keys = conn
            .query_keymap()
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let width = mapping.keycodes.len() / 8;
        for modifier in [0, 2, 3, 6] {
            // Shift, Control, Mod1 (Alt), Mod4 (Super).
            for &key in &mapping.keycodes[modifier * width..(modifier + 1) * width] {
                if key != 0 && keys.keys[(key / 8) as usize] & (1 << (key % 8)) != 0 {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    // ICCCM TIMESTAMP identifies the most recent selection acquisition even
    // when the application copies the same bytes again from the same window.
    // Zero means unsupported: callers must retain conservative text fallback.
    pub fn sequence() -> u64 {
        sequence_inner().unwrap_or(0)
    }
    fn sequence_inner() -> Option<u64> {
        use x11rb::protocol::Event;
        let (conn, screen) = x11rb::connect(None).ok()?;
        let selection = conn
            .intern_atom(false, b"CLIPBOARD")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let timestamp = conn
            .intern_atom(false, b"TIMESTAMP")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let property = conn
            .intern_atom(false, b"CLIPFORGE_TIMESTAMP")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let owner = conn
            .get_selection_owner(selection)
            .ok()?
            .reply()
            .ok()?
            .owner;
        if owner == 0 {
            return Some(0);
        }
        let window = conn.generate_id().ok()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            conn.setup().roots[screen].root,
            0,
            0,
            1,
            1,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            0,
            &xproto::CreateWindowAux::new(),
        )
        .ok()?
        .check()
        .ok()?;
        conn.convert_selection(window, selection, timestamp, property, x11rb::CURRENT_TIME)
            .ok()?;
        conn.flush().ok()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(120);
        while std::time::Instant::now() < deadline {
            if let Some(Event::SelectionNotify(event)) = conn.poll_for_event().ok()? {
                if event.requestor == window && event.selection == selection {
                    if event.property == 0 {
                        return None;
                    }
                    let value = conn
                        .get_property(false, window, property, xproto::AtomEnum::ANY, 0, 1)
                        .ok()?
                        .reply()
                        .ok()?;
                    let time = value.value32()?.next()?;
                    return Some(((owner as u64) << 32) | time as u64);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        None
        // Dropping the connection destroys the temporary requestor window.
    }
    pub fn target() -> usize {
        let Ok((conn, screen)) = x11rb::connect(None) else {
            return 0;
        };
        let root = conn.setup().roots[screen].root;
        let Ok(cookie) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") else {
            return 0;
        };
        let Ok(atom) = cookie.reply() else {
            return 0;
        };
        let Ok(cookie) = conn.get_property(false, root, atom.atom, xproto::AtomEnum::WINDOW, 0, 1)
        else {
            return 0;
        };
        let Ok(reply) = cookie.reply() else {
            return 0;
        };
        reply
            .value32()
            .and_then(|mut values| values.next())
            .unwrap_or(0) as usize
    }
    pub fn focus(window: usize) {
        let Ok((conn, screen)) = x11rb::connect(None) else {
            return;
        };
        let root = conn.setup().roots[screen].root;
        let Ok(cookie) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") else {
            return;
        };
        let Ok(atom) = cookie.reply() else {
            return;
        };
        let event = xproto::ClientMessageEvent::new(32, window as u32, atom.atom, [2, 0, 0, 0, 0]);
        let _ = conn.send_event(
            false,
            root,
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        );
        let _ = conn.flush();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void};
    type Obj = *mut c_void;
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
    pub fn modifiers_released() -> bool {
        // Combined session state; left/right Command, Shift, Option, Control.
        [54, 55, 56, 60, 58, 61, 59, 62]
            .into_iter()
            .all(|key| !unsafe { CGEventSourceKeyState(0, key) })
    }
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Obj;
        fn sel_registerName(name: *const c_char) -> Obj;
        fn objc_msgSend();
    }
    unsafe fn object(receiver: Obj, selector: &'static std::ffi::CStr) -> Obj {
        let send: unsafe extern "C" fn(Obj, Obj) -> Obj =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        unsafe { send(receiver, sel_registerName(selector.as_ptr())) }
    }
    pub fn target() -> usize {
        unsafe {
            let workspace = object(objc_getClass(c"NSWorkspace".as_ptr()), c"sharedWorkspace");
            let application = object(workspace, c"frontmostApplication");
            let send: unsafe extern "C" fn(Obj, Obj) -> i32 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            send(application, sel_registerName(c"processIdentifier".as_ptr())) as usize
        }
    }
    pub fn focus(pid: usize) {
        unsafe {
            let lookup: unsafe extern "C" fn(Obj, Obj, i32) -> Obj =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let app = lookup(
                objc_getClass(c"NSRunningApplication".as_ptr()),
                sel_registerName(c"runningApplicationWithProcessIdentifier:".as_ptr()),
                pid as i32,
            );
            let activate: unsafe extern "C" fn(Obj, Obj, usize) -> bool =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let _ = activate(app, sel_registerName(c"activateWithOptions:".as_ptr()), 2);
        }
    }
    pub fn sequence() -> u64 {
        unsafe {
            let pasteboard = object(
                objc_getClass(c"NSPasteboard".as_ptr()),
                c"generalPasteboard",
            );
            let count: unsafe extern "C" fn(Obj, Obj) -> isize =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            count(pasteboard, sel_registerName(c"changeCount".as_ptr())) as u64
        }
    }
}
