//! Global hold-to-talk key detection.
//!
//! macOS: a listen-only CGEventTap that reads modifier flags from FlagsChanged events. We never
//! ask the OS for the character of a key, which is what made the `rdev` crate crash inside
//! Text Input Services on recent macOS versions.
//!
//! Windows/Linux: `rdev`, which wraps a low-level keyboard hook and gives us key down/up.

use anyhow::{anyhow, Result};
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation::runloop::CFRunLoop;
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, CallbackResult,
    };
    use std::sync::Mutex;

    pub type Key = CGEventFlags;

    /// On macOS the hotkey is a set of modifier flags; left/right variants are treated alike.
    pub fn parse_key(name: &str) -> Result<Key> {
        Ok(match name {
            "Function" | "Fn" => CGEventFlags::CGEventFlagSecondaryFn,
            "ControlLeft" | "ControlRight" | "Control" => CGEventFlags::CGEventFlagControl,
            "MetaLeft" | "MetaRight" | "Meta" | "CommandLeft" | "CommandRight" | "Command" => {
                CGEventFlags::CGEventFlagCommand
            }
            "Alt" | "AltGr" | "AltLeft" | "AltRight" | "Option" | "OptionLeft" | "OptionRight" => {
                CGEventFlags::CGEventFlagAlternate
            }
            "ShiftLeft" | "ShiftRight" | "Shift" => CGEventFlags::CGEventFlagShift,
            "CapsLock" => CGEventFlags::CGEventFlagAlphaShift,
            other => return Err(anyhow!("unsupported hotkey name on macOS: {other} (use modifier keys)")),
        })
    }

    pub fn spawn(combo: Vec<Key>, tx: Sender<HotkeyEvent>) {
        let mut required = CGEventFlags::empty();
        for k in combo {
            required |= k;
        }
        std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || {
                let active = Mutex::new(false);
                let result = CGEventTap::with_enabled(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    vec![CGEventType::FlagsChanged, CGEventType::KeyDown, CGEventType::KeyUp],
                    |_proxy, _etype, event| {
                        let flags = event.get_flags();
                        let all_held = flags.contains(required);
                        let mut active = active.lock().unwrap();
                        if all_held && !*active {
                            *active = true;
                            let _ = tx.send(HotkeyEvent::Down);
                        } else if !all_held && *active {
                            *active = false;
                            let _ = tx.send(HotkeyEvent::Up);
                        }
                        CallbackResult::Keep
                    },
                    || CFRunLoop::run_current(),
                );
                if result.is_err() {
                    log::error!(
                        "could not install the global key listener. Grant Accessibility and Input \
                         Monitoring to this app (or to your terminal when running in dev), then restart."
                    );
                }
            })
            .expect("spawn hotkey thread");
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    use rdev::{Event, EventType};
    use std::collections::HashSet;

    pub type Key = rdev::Key;

    pub fn parse_key(name: &str) -> Result<Key> {
        use rdev::Key::*;
        Ok(match name {
            "Function" | "Fn" => Function,
            "ControlLeft" | "Control" => ControlLeft,
            "ControlRight" => ControlRight,
            "MetaLeft" | "Meta" | "WinLeft" | "CommandLeft" => MetaLeft,
            "MetaRight" | "WinRight" | "CommandRight" => MetaRight,
            "Alt" | "AltLeft" | "OptionLeft" => Alt,
            "AltGr" | "AltRight" | "OptionRight" => AltGr,
            "ShiftLeft" | "Shift" => ShiftLeft,
            "ShiftRight" => ShiftRight,
            "CapsLock" => CapsLock,
            other => return Err(anyhow!("unsupported hotkey name: {other}")),
        })
    }

    pub fn spawn(combo: Vec<Key>, tx: Sender<HotkeyEvent>) {
        std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || {
                let mut held: HashSet<Key> = HashSet::new();
                let mut active = false;
                let callback = move |event: Event| {
                    match event.event_type {
                        EventType::KeyPress(k) => {
                            held.insert(k);
                        }
                        EventType::KeyRelease(k) => {
                            held.remove(&k);
                        }
                        _ => return,
                    }
                    let all_held = combo.iter().all(|k| held.contains(k));
                    if all_held && !active {
                        active = true;
                        let _ = tx.send(HotkeyEvent::Down);
                    } else if !all_held && active {
                        active = false;
                        let _ = tx.send(HotkeyEvent::Up);
                    }
                };
                if let Err(e) = rdev::listen(callback) {
                    log::error!("global key listener failed: {e:?}");
                }
            })
            .expect("spawn hotkey thread");
    }
}

pub use imp::{parse_key, spawn};
