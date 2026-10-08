//! Platform independent shortcut state machine. Feed physical events only.
use std::time::{Duration, Instant};

const PREFIX: Duration = Duration::from_secs(2);
const C: u16 = 0x43;
const V: u16 = 0x56;
const ESC: u16 = 0x1b;
const SPACE: u16 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Copy { register: char, target: usize },
    Paste { register: char, target: usize },
    Toggle { target: usize },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub suppress: bool,
    pub action: Option<Action>,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    copy: bool,
    target: usize,
    started: Instant,
    released: bool,
}

#[derive(Debug)]
pub struct Input {
    pending: Option<Pending>,
    // Keep swallowed downs paired with swallowed ups even after cancellation.
    swallowed: [bool; 256],
}

impl Default for Input {
    fn default() -> Self {
        Self {
            pending: None,
            swallowed: [false; 256],
        }
    }
}

impl Input {
    pub fn cancel_prefix(&mut self) {
        self.pending = None;
    }
    pub fn tick(&mut self, now: Instant) -> Option<Action> {
        let p = self.pending?;
        let elapsed = now.saturating_duration_since(p.started);
        if elapsed >= PREFIX {
            self.pending = None;
        }
        None
    }

    pub fn handle(
        &mut self,
        key: u16,
        down: bool,
        ctrl: bool,
        alt: bool,
        now: Instant,
        target: usize,
    ) -> Decision {
        let index = usize::from(key);
        if index >= self.swallowed.len() {
            return Decision::default();
        }
        let paired_suppress = if down {
            self.swallowed[index]
        } else {
            std::mem::replace(&mut self.swallowed[index], false)
        };
        if !down {
            if let Some(p) = self.pending.as_mut()
                && key == if p.copy { C } else { V }
            {
                p.released = true;
            }
            return Decision {
                suppress: paired_suppress,
                action: None,
            };
        }
        if paired_suppress {
            return Decision {
                suppress: true,
                action: None,
            };
        }
        self.tick(now);
        if ctrl && alt && key == SPACE {
            self.pending = None;
            self.swallowed[index] = true;
            return Decision {
                suppress: true,
                action: Some(Action::Toggle { target }),
            };
        }
        if let Some(p) = self.pending {
            if key == ESC {
                self.pending = None;
                self.swallowed[index] = true;
                return Decision {
                    suppress: true,
                    action: None,
                };
            }
            // Modifier changes are allowed while releasing the prefix.
            if matches!(key, 0x10..=0x12 | 0xa0..=0xa5) {
                return Decision::default();
            }
            if p.released && (0x41..=0x5a).contains(&key) {
                self.pending = None;
                self.swallowed[index] = true;
                let register = (key as u8 as char).to_ascii_lowercase();
                let action = if p.copy {
                    Action::Copy {
                        register,
                        target: p.target,
                    }
                } else {
                    Action::Paste {
                        register,
                        target: p.target,
                    }
                };
                return Decision {
                    suppress: true,
                    action: Some(action),
                };
            }
            self.pending = None;
        }
        if ctrl && alt && matches!(key, C | V) {
            self.pending = Some(Pending {
                copy: key == C,
                target,
                started: now,
                released: false,
            });
            self.swallowed[index] = true;
            return Decision {
                suppress: true,
                action: None,
            };
        }
        Decision::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_and_paste_prefixes_never_open_a_menu() {
        for key in [C, V] {
            let mut i = Input::default();
            let t = Instant::now();
            assert!(i.handle(key, true, true, true, t, 7).suppress);
            assert_eq!(i.tick(t + Duration::from_millis(500)), None);
            assert!(i.handle(key, true, true, true, t, 7).suppress);
            // Releasing modifiers before C/V is permitted as well.
            assert_eq!(i.handle(0xa2, false, false, true, t, 7).action, None);
            assert!(i.handle(key, false, false, false, t, 7).suppress);
            let expected = if key == C {
                Action::Copy {
                    register: 'x',
                    target: 7,
                }
            } else {
                Action::Paste {
                    register: 'x',
                    target: 7,
                }
            };
            assert_eq!(
                i.handle(0x58, true, false, false, t, 9).action,
                Some(expected)
            );
            assert!(i.handle(0x58, true, false, false, t, 9).suppress);
            assert!(i.handle(0x58, false, false, false, t, 9).suppress);
            assert!(!i.handle(0x58, true, false, false, t, 9).suppress);
        }
    }
    #[test]
    fn space_toggles_once_per_press_and_modifier_release_does_nothing() {
        let mut i = Input::default();
        let t = Instant::now();
        assert_eq!(
            i.handle(SPACE, true, true, true, t, 7).action,
            Some(Action::Toggle { target: 7 })
        );
        assert!(i.handle(SPACE, true, true, true, t, 7).suppress);
        assert_eq!(i.handle(SPACE, true, true, true, t, 7).action, None);
        assert_eq!(i.handle(0xa2, false, false, true, t, 7).action, None);
        assert!(i.handle(SPACE, false, false, false, t, 7).suppress);
        assert_eq!(i.tick(t + Duration::from_secs(3)), None);
        assert_eq!(
            i.handle(SPACE, true, true, true, t, 9).action,
            Some(Action::Toggle { target: 9 })
        );
    }
    #[test]
    fn space_cancels_pending_prefix_and_plain_shortcuts_pass_through() {
        let mut i = Input::default();
        let t = Instant::now();
        i.handle(C, true, true, true, t, 7);
        i.handle(C, false, true, true, t, 7);
        i.handle(SPACE, true, true, true, t, 7);
        assert_eq!(
            i.handle(0x58, true, false, false, t, 7),
            Decision::default()
        );
        for key in [C, V, SPACE] {
            assert_eq!(
                Input::default().handle(key, true, true, false, t, 0),
                Decision::default()
            );
        }
    }
    #[test]
    fn timeout_escape_and_invalid_keys_cancel_prefix() {
        for (key, delay) in [
            (ESC, Duration::ZERO),
            (0x31, Duration::ZERO),
            (0x58, PREFIX),
        ] {
            let mut i = Input::default();
            let t = Instant::now();
            i.handle(V, true, true, true, t, 7);
            i.handle(V, false, false, false, t, 7);
            assert_eq!(i.handle(key, true, false, false, t + delay, 7).action, None);
            assert_eq!(
                i.handle(0x59, true, false, false, t + delay, 7).action,
                None
            );
        }
    }
    #[test]
    fn register_can_be_selected_with_modifiers_still_held() {
        let mut i = Input::default();
        let t = Instant::now();
        i.handle(V, true, true, true, t, 7);
        i.handle(V, false, true, true, t, 7);
        assert_eq!(
            i.handle(C, true, true, true, t, 9).action,
            Some(Action::Paste {
                register: 'c',
                target: 7
            })
        );
    }
}
