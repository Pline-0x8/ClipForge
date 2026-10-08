use crate::{
    core::Engine,
    platform::{self, Event},
};
use arboard::Clipboard;
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum Command {
    Platform(Event),
    Load(String),
    SetClipboard {
        text: String,
        reply: Sender<Result<(), String>>,
    },
    Select {
        register: char,
        copy: bool,
        target: usize,
    },
    SaveCurrent(char),
    SaveText {
        register: char,
        text: String,
    },
    EditRegister {
        register: char,
        name: String,
        text: String,
    },
    ClearRegister(char),
    ClearHistory,
    ClearRegisters,
    Clear,
}
#[derive(Debug)]
pub enum Update {
    Snapshot(Box<Engine>),
    Clipboard(Option<String>),
    Show { copy: bool, target: usize },
    Dismiss { target: usize, commit: bool },
    Status(String),
    Toggle { target: usize },
}

const MAX_TEXT: usize = 1024 * 1024;
pub fn validate_edit(register: char, name: &str, text: &str) -> Result<(), String> {
    crate::core::register_index(register).map_err(|e| e.to_string())?;
    if name.chars().count() > 80 {
        return Err("Register name exceeds 80 characters".into());
    }
    validate_text(text)
}
pub fn validate_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_TEXT {
        return Err("Text exceeds the 1 MiB entry limit".into());
    }
    Ok(())
}
fn read_text(clipboard: &mut Clipboard) -> Result<String, String> {
    let text = clipboard.get_text().map_err(|e| e.to_string())?;
    if text.len() > MAX_TEXT {
        return Err("Text exceeds the 1 MiB entry limit".into());
    }
    Ok(text)
}
// None skips a transient read failure; Some(None) means no supported text.
fn monitor_text(result: Result<String, arboard::Error>) -> Option<Option<String>> {
    match result {
        Ok(text) if text.len() <= MAX_TEXT => Some(Some(text)),
        Ok(_) | Err(arboard::Error::ContentNotAvailable) => Some(None),
        Err(_) => None,
    }
}
fn read_current(clipboard: &mut Clipboard) -> Option<Option<String>> {
    // Some Windows read failures are reported as missing content, even when
    // another clipboard reader merely caused a transient native read failure.
    for attempt in 0..3 {
        match clipboard.get_text() {
            Err(arboard::Error::ContentNotAvailable) if attempt < 2 => {
                thread::sleep(Duration::from_millis(15));
            }
            result => return monitor_text(result),
        }
    }
    unreachable!()
}
fn write_text(clipboard: &mut Clipboard, text: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match clipboard.set_text(text) {
            Ok(()) => return Ok(()),
            Err(e) if Instant::now() >= deadline => return Err(e.to_string()),
            Err(_) => thread::sleep(Duration::from_millis(15)),
        }
    }
}
fn copy_text(clipboard: &mut Clipboard, target: usize) -> Result<String, String> {
    let before = clipboard.get_text().ok();
    let before_sequence = platform::native_shortcut(true, target)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let changed = if cfg!(target_os = "linux") {
            let current_sequence = platform::sequence();
            (before_sequence != 0 && current_sequence != 0 && current_sequence != before_sequence)
                || clipboard
                    .get_text()
                    .ok()
                    .is_some_and(|text| Some(text) != before)
        } else {
            platform::sequence() != before_sequence
        };
        if changed && let Ok(text) = read_text(clipboard) {
            return Ok(text);
        }
        if Instant::now() >= deadline {
            return Err("Copy did not produce readable new text. Check selection and VM clipboard sharing; identical text on X11 can be saved using Save current.".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn run(rx: Receiver<Command>, tx: Sender<Update>) {
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(Update::Status(format!("Clipboard unavailable: {e}")));
            return;
        }
    };
    let mut engine = Engine::default();
    let mut last_text: Option<String> = None;
    let mut last_clipboard: Option<String> = None;
    let mut prepared: Option<String> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(120)) {
            Ok(command) => {
                let result: Result<(), String> = (|| {
                    match command {
                        Command::Platform(Event::Copy { register, target }) => {
                            let text = copy_text(&mut clipboard, target)?;
                            engine
                                .save_register(register, &text)
                                .map_err(|e| e.to_string())?;
                            last_text = Some(text);
                            let _ = tx.send(Update::Status(format!(
                                "Saved register {}",
                                register.to_ascii_lowercase()
                            )));
                        }
                        Command::Platform(Event::Paste { register, target })
                        | Command::Select {
                            register,
                            copy: false,
                            target,
                        } => {
                            let text = engine.registers()[crate::core::register_index(register)
                                .map_err(|e| e.to_string())?]
                            .clone()
                            .ok_or_else(|| format!("Register {register} is empty"))?;
                            write_text(&mut clipboard, &text)?;
                            engine.load_register(register).map_err(|e| e.to_string())?;
                            last_text = Some(text);
                            platform::native_shortcut(false, target)?;
                        }
                        Command::Platform(Event::PrepareCopy { target })
                        | Command::Platform(Event::Show { copy: true, target }) => {
                            prepared = None;
                            let text = copy_text(&mut clipboard, target)?;
                            engine.observe(&text);
                            last_text = Some(text.clone());
                            prepared = Some(text);
                            let _ = tx.send(Update::Show { copy: true, target });
                        }
                        Command::Platform(Event::Show {
                            copy: false,
                            target,
                        }) => {
                            prepared = None;
                            let _ = tx.send(Update::Show {
                                copy: false,
                                target,
                            });
                        }
                        Command::Platform(Event::Toggle { target }) => {
                            let _ = tx.send(Update::Toggle { target });
                        }
                        Command::Platform(Event::Dismiss { target, commit }) => {
                            let _ = tx.send(Update::Dismiss { target, commit });
                        }
                        Command::Select {
                            register,
                            copy: true,
                            ..
                        } => {
                            let text = prepared
                                .take()
                                .ok_or("No copied text is ready; copy again")?;
                            engine
                                .save_register(register, &text)
                                .map_err(|e| e.to_string())?;
                        }
                        Command::Load(text) => {
                            validate_text(&text)?;
                            write_text(&mut clipboard, &text)?;
                            engine.observe(&text);
                            last_text = Some(text);
                        }
                        Command::SetClipboard { text, reply } => {
                            let result = validate_text(&text)
                                .and_then(|()| write_text(&mut clipboard, &text))
                                .map(|()| {
                                    engine.observe(&text);
                                    last_text = Some(text);
                                });
                            let _ = reply.send(result.clone());
                            result?;
                        }
                        Command::SaveText { register, text } => {
                            if text.len() > MAX_TEXT {
                                return Err("Text exceeds the 1 MiB entry limit".into());
                            }
                            engine
                                .save_register(register, &text)
                                .map_err(|e| e.to_string())?;
                            let _ = tx.send(Update::Status(format!(
                                "Saved register {}",
                                register.to_ascii_lowercase()
                            )));
                        }
                        Command::SaveCurrent(register) => {
                            let text = read_text(&mut clipboard)?;
                            engine
                                .save_register(register, &text)
                                .map_err(|e| e.to_string())?;
                            last_text = Some(text);
                        }
                        Command::EditRegister {
                            register,
                            name,
                            text,
                        } => {
                            validate_edit(register, &name, &text)?;
                            engine
                                .edit_register(register, &name, &text)
                                .map_err(|e| e.to_string())?;
                            let _ = tx.send(Update::Status(format!(
                                "Updated register {}",
                                register.to_ascii_lowercase()
                            )));
                        }
                        Command::ClearRegister(register) => {
                            engine.clear_register(register).map_err(|e| e.to_string())?;
                            let _ = tx.send(Update::Status(format!(
                                "Cleared register {}",
                                register.to_ascii_lowercase()
                            )));
                        }
                        Command::ClearRegisters => {
                            engine.clear_registers();
                            prepared = None;
                            let _ = tx.send(Update::Status("Registers cleared".into()));
                        }
                        Command::ClearHistory => {
                            engine.clear_history();
                            let _ = tx.send(Update::Status("History cleared".into()));
                        }
                        Command::Clear => {
                            // Clear the OS first; failure must not report successful clear-all.
                            write_text(&mut clipboard, "")?;
                            engine.clear();
                            prepared = None;
                            last_text = Some(String::new());
                            let _ = tx.send(Update::Status(
                                "All registers, history and host clipboard cleared".into(),
                            ));
                        }
                        Command::Platform(Event::Error) => {
                            return Err("Platform shortcut error".into());
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let _ = tx.send(Update::Status(error));
                }
                let _ = tx.send(Update::Snapshot(Box::new(engine.clone())));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        let Some(current) = read_current(&mut clipboard) else {
            continue;
        };
        if current != last_text {
            if let Some(text) = &current {
                engine.observe(text);
            }
            last_text = current.clone();
            let _ = tx.send(Update::Snapshot(Box::new(engine.clone())));
        }
        if current != last_clipboard {
            last_clipboard = current.clone();
            let _ = tx.send(Update::Clipboard(current));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_read_failure_preserves_current_clipboard_instead_of_reporting_empty() {
        assert_eq!(monitor_text(Err(arboard::Error::ClipboardOccupied)), None);
        assert_eq!(
            monitor_text(Err(arboard::Error::ContentNotAvailable)),
            Some(None)
        );
        assert_eq!(monitor_text(Ok(String::new())), Some(Some(String::new())));
        assert_eq!(
            monitor_text(Ok("text 🌍".into())),
            Some(Some("text 🌍".into()))
        );
        assert_eq!(monitor_text(Ok("x".repeat(MAX_TEXT + 1))), Some(None));
    }
}
