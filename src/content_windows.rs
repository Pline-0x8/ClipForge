//! Only independently owned HGLOBAL formats are retained; GDI/OLE handles are skipped.
use super::{Content, Format, MAX_ENTRY_BYTES, describe};
use std::{
    ptr::{null, null_mut},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{DataExchange::*, Memory::*},
    UI::WindowsAndMessaging::*,
};

struct Open;
static ACCESS: Mutex<()> = Mutex::new(());
pub fn read_text() -> Result<Option<String>, String> {
    let _access = ACCESS.lock().map_err(|_| "Clipboard access lock failed")?;
    let _open = open(owner()?)?;
    if unsafe { IsClipboardFormatAvailable(13) } == 0 {
        return Ok(None);
    }
    let handle = unsafe { GetClipboardData(13) };
    if handle.is_null() {
        return Err("Clipboard text is temporarily unavailable".into());
    }
    let size = unsafe { GlobalSize(handle) };
    if size > 2 * 1024 * 1024 {
        return Ok(None);
    }
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        return Err("Could not lock clipboard text".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec() };
    unsafe {
        GlobalUnlock(handle);
    }
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    let text = String::from_utf16(&units).map_err(|_| "Clipboard text contains invalid Unicode")?;
    Ok((text.len() <= 1024 * 1024).then_some(text))
}

pub fn write_text(text: &str) -> Result<(), String> {
    let bytes = text
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    restore(&Content {
        formats: vec![Format {
            id: 13,
            name: "Unicode text".into(),
            bytes,
        }],
        view: super::EntryView::text(text),
    })
}
impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
fn open(window: HWND) -> Result<Open, String> {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        if unsafe { OpenClipboard(window) } != 0 {
            return Ok(Open);
        }
        if Instant::now() >= deadline {
            return Err("Clipboard is busy; try again".into());
        }
        thread::sleep(Duration::from_millis(15));
    }
}

pub fn capture() -> Result<Option<Content>, String> {
    let _access = ACCESS.lock().map_err(|_| "Clipboard access lock failed")?;
    let _open = open(owner()?)?;
    let mut formats = Vec::new();
    let mut total = 0usize;
    let mut id = 0;
    loop {
        id = unsafe { EnumClipboardFormats(id) };
        if id == 0 {
            break;
        }
        // Bitmap, palette and metafile handles cannot be copied as global bytes.
        if !matches!(id, 1 | 4 | 5 | 6 | 7 | 8 | 10 | 11 | 13 | 15 | 16 | 17)
            && !(0xc000..=0xffff).contains(&id)
        {
            continue;
        }
        let handle = unsafe { GetClipboardData(id) };
        if handle.is_null() {
            continue;
        }
        let size = unsafe { GlobalSize(handle) };
        if size == 0 {
            continue;
        }
        total = total
            .checked_add(size)
            .ok_or("Clipboard data is too large")?;
        if total > MAX_ENTRY_BYTES {
            return Err("Clipboard entry exceeds the 32 MiB capture limit".into());
        }
        let data = unsafe { GlobalLock(handle) };
        if data.is_null() {
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec() };
        unsafe {
            GlobalUnlock(handle);
        }
        let name = match id {
            13 => "Unicode text".into(),
            15 => "Files (HDROP)".into(),
            8 => "DIB image".into(),
            17 => "DIBV5 image".into(),
            _ => {
                let mut buffer = [0u16; 256];
                let len = unsafe {
                    GetClipboardFormatNameW(id, buffer.as_mut_ptr(), buffer.len() as i32)
                };
                if len > 0 {
                    String::from_utf16_lossy(&buffer[..len as usize])
                } else {
                    format!("Clipboard format {id}")
                }
            }
        };
        // Shell bookkeeping is process-specific and may contain stale object references.
        if [
            "Shell IDList Array",
            "DataObject",
            "Ole Private Data",
            "ObjectLink",
            "OwnerLink",
            "FileContents",
            "FileGroupDescriptor",
            "FileGroupDescriptorW",
        ]
        .contains(&name.as_str())
        {
            continue;
        }
        formats.push(Format { id, name, bytes });
        if formats.len() > 128 {
            return Err("Clipboard has too many formats".into());
        }
    }
    drop(_open);
    Ok((!formats.is_empty()).then(|| describe(formats)))
}

static OWNER: OnceLock<Result<usize, String>> = OnceLock::new();
fn owner() -> Result<HWND, String> {
    // Clipboard ownership messages need a live message pump, even for eager formats.
    let owner = OWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            if hwnd.is_null() {
                let _ = tx.send(Err("Could not create clipboard owner window".into()));
                return;
            }
            let _ = tx.send(Ok(hwnd as usize));
            unsafe {
                let mut message = std::mem::zeroed();
                while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                DestroyWindow(hwnd);
            }
        });
        rx.recv()
            .unwrap_or_else(|_| Err("Clipboard owner thread stopped".into()))
    });
    owner.clone().map(|hwnd| hwnd as HWND)
}

struct Allocation {
    id: u32,
    handle: windows_sys::Win32::Foundation::HGLOBAL,
}
impl Drop for Allocation {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                GlobalFree(self.handle);
            }
        }
    }
}

pub fn restore(content: &Content) -> Result<(), String> {
    let _access = ACCESS.lock().map_err(|_| "Clipboard access lock failed")?;
    if content.formats.is_empty() {
        return Err("This clipboard entry has no restorable formats".into());
    }
    // Allocate everything before modifying the host clipboard.
    let mut blocks = Vec::new();
    for format in &content.formats {
        let mut bytes = format.bytes.clone();
        if content.view.kind == "files" && format.name == "Preferred DropEffect" {
            bytes = 1u32.to_le_bytes().to_vec();
        }
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len().max(1)) };
        if handle.is_null() {
            return Err("Could not allocate clipboard data".into());
        }
        let block = Allocation {
            id: format.id,
            handle,
        };
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            return Err("Could not lock clipboard data".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
            GlobalUnlock(handle);
        }
        blocks.push(block);
    }
    let _open = open(owner()?)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err("Could not clear host clipboard".into());
    }
    for block in &mut blocks {
        if unsafe { SetClipboardData(block.id, block.handle) }.is_null() {
            return Err(
                "Could not restore all clipboard formats; clipboard may be partially restored"
                    .into(),
            );
        }
        // Ownership transfers to Windows on success.
        block.handle = null_mut();
    }
    Ok(())
}
