//! Global hold-to-talk key detection.
//!
//! macOS: a listen-only CGEventTap that reads modifier flags from FlagsChanged events. We never
//! ask the OS for the character of a key, which is what made the `rdev` crate crash inside
//! Text Input Services on recent macOS versions.
//!
//! Windows/Linux: `rdev`, which wraps a low-level keyboard hook and gives us key down/up.

use anyhow::{anyhow, Result};
use std::sync::mpsc::Sender;

/// A key the Settings picker offers: the name stored in the config and what the user sees.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Choice {
    pub name: &'static str,
    pub label: &'static str,
}

/// The keys a hotkey can be made of, in the order Settings shows them. Only keys that are held
/// down make sense for hold-to-talk, so these are the modifiers.
#[cfg(target_os = "macos")]
pub const CHOICES: &[Choice] = &[
    Choice { name: "Function", label: "Fn" },
    Choice { name: "Shift", label: "Shift" },
    Choice { name: "Control", label: "Control" },
    Choice { name: "Option", label: "Option" },
    Choice { name: "Command", label: "Command" },
];
#[cfg(not(target_os = "macos"))]
pub const CHOICES: &[Choice] = &[
    Choice { name: "ControlLeft", label: "Left Ctrl" },
    Choice { name: "ControlRight", label: "Right Ctrl" },
    Choice { name: "MetaLeft", label: "Win" },
    Choice { name: "Alt", label: "Alt" },
    Choice { name: "AltGr", label: "AltGr" },
    Choice { name: "ShiftLeft", label: "Left Shift" },
    Choice { name: "ShiftRight", label: "Right Shift" },
];

/// The name Settings offers for a key the config may spell differently ("Fn", "ShiftLeft",
/// "CommandLeft", ...), so an existing hotkey ticks the right boxes. Names with no offered
/// equivalent come back unchanged.
pub fn canonical(name: &str) -> String {
    #[cfg(target_os = "macos")]
    let offered = match name {
        "Fn" => "Function",
        "ShiftLeft" | "ShiftRight" => "Shift",
        "ControlLeft" | "ControlRight" => "Control",
        "Alt" | "AltGr" | "AltLeft" | "AltRight" | "OptionLeft" | "OptionRight" => "Option",
        "Meta" | "MetaLeft" | "MetaRight" | "CommandLeft" | "CommandRight" => "Command",
        other => other,
    };
    #[cfg(not(target_os = "macos"))]
    let offered = match name {
        "Fn" => "Function",
        "Control" => "ControlLeft",
        "Meta" | "WinLeft" | "CommandLeft" => "MetaLeft",
        "AltLeft" | "OptionLeft" => "Alt",
        "AltRight" | "OptionRight" => "AltGr",
        "Shift" => "ShiftLeft",
        other => other,
    };
    offered.to_string()
}

/// A configured hotkey in the picker's names: what Settings ticks, and the starting point of any
/// edit there, so a hand-written "Fn" is still part of the hotkey after a key is added.
pub fn picker_names(configured: &[String]) -> Vec<String> {
    configured.iter().map(|k| canonical(k)).collect()
}

/// How a hotkey reads to a person, e.g. "Fn + Shift". Names the picker does not offer (written
/// into the config by hand) are shown as they are.
pub fn label(names: &[String]) -> String {
    names
        .iter()
        .map(|n| {
            let n = canonical(n);
            CHOICES.iter().find(|c| c.name == n).map_or(n, |c| c.label.to_string())
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Checks a hotkey before it is used: every key must exist on this platform, and it must not
/// fire during ordinary typing.
pub fn validate(names: &[String]) -> Result<()> {
    if names.is_empty() {
        return Err(anyhow!("pick at least one key"));
    }
    for n in names {
        parse_key(n)?;
    }
    let only_shift = names.iter().all(|n| n.starts_with("Shift"));
    if only_shift {
        return Err(anyhow!(
            "Shift on its own would start dictating on every capital letter; add another key"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, CallbackResult,
    };
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    pub type Key = CGEventFlags;

    /// The flags that must all be held, read by the event tap on every event so Settings can
    /// change the hotkey without a restart. Zero means no hotkey.
    static REQUIRED: AtomicU64 = AtomicU64::new(0);

    pub fn set_combo(combo: Vec<Key>) {
        let required = combo.into_iter().fold(CGEventFlags::empty(), |a, k| a | k);
        REQUIRED.store(required.bits(), AtomicOrdering::SeqCst);
    }

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
        set_combo(combo);
        std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || {
                use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
                use std::sync::atomic::{AtomicBool, Ordering};
                use std::sync::Arc;

                let active = Arc::new(Mutex::new(false));
                let needs_reenable = Arc::new(AtomicBool::new(false));
                let reenable_flag = needs_reenable.clone();

                // Only modifier-flag changes matter for a modifier hotkey. Key up/down events
                // can carry flags without the Fn bit and would read as a false release.
                let tap = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    vec![CGEventType::FlagsChanged],
                    move |_proxy, etype, event| {
                        match etype {
                            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                                log::warn!("event tap disabled by macOS ({etype:?}); re-enabling");
                                reenable_flag.store(true, Ordering::SeqCst);
                                return CallbackResult::Keep;
                            }
                            _ => {}
                        }
                        let flags = event.get_flags();
                        let required =
                            CGEventFlags::from_bits_truncate(REQUIRED.load(AtomicOrdering::SeqCst));
                        let all_held = !required.is_empty() && flags.contains(required);
                        log::debug!(
                            "tap event {:?} keycode {} flags {:#x} held={all_held}",
                            etype,
                            event.get_integer_value_field(
                                core_graphics::event::EventField::KEYBOARD_EVENT_KEYCODE
                            ),
                            flags.bits()
                        );
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
                );
                let tap = match tap {
                    Ok(t) => t,
                    Err(_) => {
                        log::error!(
                            "could not install the global key listener. Grant Accessibility and Input \
                             Monitoring to this app, then restart."
                        );
                        return;
                    }
                };
                let source = match tap.mach_port().create_runloop_source(0) {
                    Ok(s) => s,
                    Err(_) => {
                        log::error!("could not create run loop source for the key listener");
                        return;
                    }
                };
                let run_loop = CFRunLoop::get_current();
                run_loop.add_source(&source, unsafe { kCFRunLoopDefaultMode });
                tap.enable();
                log::info!("global key listener installed");
                loop {
                    CFRunLoop::run_in_mode(
                        unsafe { kCFRunLoopDefaultMode },
                        std::time::Duration::from_millis(500),
                        false,
                    );
                    if needs_reenable.swap(false, Ordering::SeqCst) {
                        tap.enable();
                    }
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
    use std::sync::Mutex;

    pub type Key = rdev::Key;

    /// The keys that must all be held, read on every key event so Settings can change the hotkey
    /// without a restart. Empty means no hotkey.
    static COMBO: Mutex<Vec<Key>> = Mutex::new(Vec::new());

    pub fn set_combo(combo: Vec<Key>) {
        *COMBO.lock().unwrap() = combo;
    }

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
        set_combo(combo);
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
                    let all_held = {
                        let combo = COMBO.lock().unwrap();
                        !combo.is_empty() && combo.iter().all(|k| held.contains(k))
                    };
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

pub use imp::{parse_key, set_combo, spawn};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_key_parses() {
        for c in CHOICES {
            parse_key(c.name).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        }
    }

    #[test]
    fn shift_alone_is_refused() {
        let shift = CHOICES.iter().find(|c| c.name.starts_with("Shift")).unwrap().name;
        assert!(validate(&[shift.to_string()]).is_err());
        assert!(validate(&[]).is_err());
        assert!(validate(&["Nope".to_string()]).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hand_written_names_tick_the_offered_boxes() {
        let names: Vec<String> = ["Fn", "ShiftLeft", "CommandRight"].iter().map(|n| canonical(n)).collect();
        assert_eq!(names, ["Function", "Shift", "Command"]);
        assert!(names.iter().all(|n| CHOICES.iter().any(|c| c.name == *n)));
        assert_eq!(canonical("CapsLock"), "CapsLock", "unknown names are kept");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn adding_a_key_to_a_hand_written_hotkey_keeps_its_keys() {
        // Settings ticks picker_names, the user ticks Control too, and Apply sends the ticked boxes.
        let configured = vec!["Fn".to_string(), "ShiftLeft".to_string()];
        let mut edited = picker_names(&configured);
        edited.push("Control".to_string());
        assert_eq!(edited, ["Function", "Shift", "Control"]);
        assert!(edited.iter().all(|n| CHOICES.iter().any(|c| c.name == *n)), "every key has a box");
        assert!(validate(&edited).is_ok());
        assert_eq!(label(&edited), "Fn + Shift + Control");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn adding_a_key_to_a_hand_written_hotkey_keeps_its_keys() {
        let configured = vec!["Control".to_string(), "WinLeft".to_string()];
        let mut edited = picker_names(&configured);
        edited.push("ShiftLeft".to_string());
        assert_eq!(edited, ["ControlLeft", "MetaLeft", "ShiftLeft"]);
        assert!(edited.iter().all(|n| CHOICES.iter().any(|c| c.name == *n)), "every key has a box");
        assert!(validate(&edited).is_ok());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn hand_written_names_tick_the_offered_boxes() {
        let names: Vec<String> = ["Control", "Shift", "WinLeft"].iter().map(|n| canonical(n)).collect();
        assert_eq!(names, ["ControlLeft", "ShiftLeft", "MetaLeft"]);
        assert!(names.iter().all(|n| CHOICES.iter().any(|c| c.name == *n)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fn_and_shift_read_well() {
        let combo = vec!["Function".to_string(), "Shift".to_string()];
        assert!(validate(&combo).is_ok());
        assert_eq!(label(&combo), "Fn + Shift");
        assert_eq!(label(&["Fn".to_string(), "ShiftLeft".to_string()]), "Fn + Shift");
    }
}
