#![cfg(windows)]
//! Native fixtures retain real format bytes; no application-specific clipboard mocking.
use clipforge::{
    content::{self, Content, Format},
    service::{self, Command, Update},
};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Restore(Option<Content>);
impl Drop for Restore {
    fn drop(&mut self) {
        if let Some(content) = &self.0 {
            let _ = content::restore(content);
        } else if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text("");
        }
    }
}
fn registered(name: &str) -> u32 {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let id = unsafe {
        windows_sys::Win32::System::DataExchange::RegisterClipboardFormatW(name.as_ptr())
    };
    assert_ne!(id, 0);
    id
}
fn text(value: &str) -> Format {
    Format {
        id: 13,
        name: "Unicode text".into(),
        bytes: value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    }
}
fn files() -> Format {
    let mut bytes = vec![0; 20];
    bytes[..4].copy_from_slice(&20u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend(
        "C:\\Fixtures\\budget.xlsx\0C:\\Fixtures\\report.pdf\0\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    Format {
        id: 15,
        name: "Files (HDROP)".into(),
        bytes,
    }
}
fn image() -> Format {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .unwrap();
    }
    Format {
        id: registered("PNG"),
        name: "PNG".into(),
        bytes,
    }
}
fn next_engine(
    rx: &mpsc::Receiver<Update>,
    predicate: impl Fn(&clipforge::core::Engine) -> bool,
) -> clipforge::core::Engine {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Update::Snapshot(engine)) = rx.recv_timeout(Duration::from_millis(50))
            && predicate(&engine)
        {
            return *engine;
        }
        assert!(
            Instant::now() < deadline,
            "Expected mixed history snapshot did not arrive"
        );
    }
}

#[test]
#[ignore = "Temporarily changes the real Windows clipboard; run individually"]
fn native_rich_history_restores_files_images_tables_and_custom_bytes() {
    let _restore = Restore(content::capture().unwrap());
    let mut clipboard = arboard::Clipboard::new().unwrap();
    clipboard.set_text("fixture baseline").unwrap();
    let (tx, commands) = mpsc::channel();
    let (updates, rx) = mpsc::channel();
    let worker = thread::spawn(move || service::run(commands, updates));
    next_engine(&rx, |engine| {
        engine.history().contains(&"fixture baseline".into())
    });
    tx.send(Command::EditRegister {
        register: 'a',
        name: "Keep label".into(),
        text: "Keep register".into(),
    })
    .unwrap();
    next_engine(&rx, |engine| engine.register_names()[0] == "Keep label");
    let fixtures = [
        (
            "files",
            vec![
                files(),
                Format {
                    id: registered("Preferred DropEffect"),
                    name: "Preferred DropEffect".into(),
                    bytes: 2u32.to_le_bytes().to_vec(),
                },
            ],
        ),
        ("image", vec![image()]),
        (
            "table",
            vec![
                text("Item\tTotal\r\nTeam\t42"),
                Format {
                    id: registered("ClipForge Test Workbook"),
                    name: "ClipForge Test Workbook".into(),
                    bytes: vec![1, 2, 3, 4, 5],
                },
            ],
        ),
        (
            "binary",
            vec![Format {
                id: registered("ClipForge Test Binary"),
                name: "ClipForge Test Binary".into(),
                bytes: vec![0xde, 0xad, 0xbe, 0xef, 0, 255],
            }],
        ),
    ];
    for (kind, formats) in fixtures {
        let source = content::describe(formats);
        content::restore(&source).unwrap();
        let engine = next_engine(&rx, |engine| {
            engine
                .entries()
                .first()
                .is_some_and(|entry| entry.view.kind == kind)
        });
        let entry = &engine.entries()[0];
        let expected = entry.content.as_ref().unwrap().clone();
        assert_eq!(engine.registers()[0].as_deref(), Some("Keep register"));
        if kind == "image" {
            assert!(
                entry
                    .view
                    .thumbnail
                    .as_ref()
                    .unwrap()
                    .starts_with("data:image/png;base64,")
            );
        }
        if kind == "files" {
            assert_eq!(entry.view.label, "budget.xlsx + 1 files");
            let effect = expected
                .formats
                .iter()
                .find(|format| format.name == "Preferred DropEffect")
                .unwrap();
            assert_eq!(
                &effect.bytes[..4],
                &1u32.to_le_bytes(),
                "Restored file references must copy rather than move"
            );
        }
        if kind == "table" {
            assert_eq!(entry.view.table[1], ["Team", "42"]);
        }
        if kind == "binary" {
            assert!(entry.view.hex.starts_with("DE AD BE EF"));
        }
        clipboard
            .set_text("replace clipboard before restoring")
            .unwrap();
        next_engine(&rx, |engine| {
            engine
                .history()
                .first()
                .is_some_and(|text| text == "replace clipboard before restoring")
        });
        let (reply, result) = mpsc::channel();
        tx.send(Command::LoadEntry {
            id: entry.view.id,
            reply,
        })
        .unwrap();
        result
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let restored = content::capture().unwrap().unwrap();
        for format in &expected.formats {
            let found = restored
                .formats
                .iter()
                .find(|f| f.id == format.id)
                .expect("Original format missing after restore");
            assert_eq!(
                found.bytes, format.bytes,
                "Original clipboard bytes changed for {}",
                format.name
            );
        }
        assert_eq!(restored.view.kind, kind);
    }
    tx.send(Command::ClearHistory).unwrap();
    next_engine(&rx, |engine| engine.entries().is_empty());
    let (reply, result) = mpsc::channel();
    tx.send(Command::LoadEntry {
        id: u64::MAX,
        reply,
    })
    .unwrap();
    assert!(
        result
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_err()
    );
    drop(tx);
    worker.join().unwrap();
}
