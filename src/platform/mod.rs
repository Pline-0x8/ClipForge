use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    PrepareCopy { target: usize },
    Toggle { target: usize },
    Copy { register: char, target: usize },
    Paste { register: char, target: usize },
    Show { copy: bool, target: usize },
    Dismiss { target: usize, commit: bool },
    Error,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(windows))]
mod portable;
#[cfg(not(windows))]
pub use portable::*;

pub fn start(tx: Sender<Event>) -> Result<(), String> {
    start_backend(tx)
}
