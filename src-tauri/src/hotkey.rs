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
    // A lone Option/AltGr types characters on many layouts (@, {, [), and a lone Command,
    // Control or Win is held for every shortcut. Fn is the one modifier that is fine alone.
    if names.len() == 1 && canonical(&names[0]) != "Function" {
        return Err(anyhow!(
            "{} on its own is held while typing and for shortcuts; add another key",
            label(names)
        ));
    }
    Ok(())
}

/// Checks the STFU switch key (see `config::StfuConfig`): empty means none; otherwise it must
/// exist here and must not be one of the hotkey's own keys, which are already held.
pub fn validate_switch(key: &str, hotkey: &[String]) -> Result<()> {
    if key.is_empty() {
        return Ok(());
    }
    // On layouts with AltGr, Windows sends a fake Left Ctrl press and release around every AltGr
    // press; the release would read as letting go of a Ctrl hotkey and end the take.
    #[cfg(not(target_os = "macos"))]
    if canonical(key) == "AltGr" {
        return Err(anyhow!("AltGr also sends a Ctrl key press on many layouts; pick another key"));
    }
    // Compared as the listener sees them, so aliases ("WinRight", "MetaRight") are one key.
    let parsed = parse_key(key)?;
    if hotkey.iter().any(|k| parse_key(k).ok() == Some(parsed)) {
        return Err(anyhow!(
            "{} is part of the hotkey, so it is already held; pick another key",
            label(&[key.to_string()])
        ));
    }
    Ok(())
}

/// The switch key as the listener wants it: `None` when there is none, or when it cannot work
/// because it is part of the hotkey.
pub fn switch_key(key: &str, hotkey: &[String]) -> Option<Key> {
    validate_switch(key, hotkey).ok()?;
    if key.is_empty() {
        return None;
    }
    parse_key(key).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
    /// Another key was pressed while the hotkey was held (Fn+Shift+Arrow selects text on a
    /// laptop): drop the take instead of dictating over the selection.
    Cancel,
    /// The switch key was pressed while the hotkey was held, or was already down when the
    /// hotkey went down: this take uses the other output style (STFU or plain clean-up).
    Switch,
}

/// Where the listener is with the hotkey. A cancelled take stays cancelled until the keys are
/// let go, so it does not restart while they are still held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    Idle,
    Active,
    Cancelled,
}

impl Hold {
    /// The hotkey's keys are (all_held) or are not all down.
    fn keys(&mut self, all_held: bool) -> Option<HotkeyEvent> {
        match (*self, all_held) {
            (Hold::Idle, true) => {
                *self = Hold::Active;
                Some(HotkeyEvent::Down)
            }
            (Hold::Active, false) => {
                *self = Hold::Idle;
                Some(HotkeyEvent::Up)
            }
            (Hold::Cancelled, false) => {
                *self = Hold::Idle;
                None
            }
            _ => None,
        }
    }

    /// A key that is not part of any hotkey went down.
    fn other_key(&mut self) -> Option<HotkeyEvent> {
        if *self == Hold::Active {
            *self = Hold::Cancelled;
            return Some(HotkeyEvent::Cancel);
        }
        None
    }
}

/// The hotkey plus the STFU switch key. Each press of the switch key during a take flips its
/// style, so a press by mistake is undone by pressing again. The keys go down in no fixed order,
/// so a switch key that is already held when the hotkey completes counts as a press too.
#[derive(Debug)]
struct Keys {
    hold: Hold,
    switch_held: bool,
}

impl Keys {
    fn new() -> Self {
        Self { hold: Hold::Idle, switch_held: false }
    }

    /// The modifier state changed: whether every hotkey key is down, and whether the switch key
    /// is. Returns the events in the order they happened.
    fn update(&mut self, all_held: bool, switch_held: bool) -> Vec<HotkeyEvent> {
        let mut out = Vec::new();
        let started = self.hold.keys(all_held);
        out.extend(started);
        let pressed = switch_held && (!self.switch_held || started == Some(HotkeyEvent::Down));
        self.switch_held = switch_held;
        if pressed && self.hold == Hold::Active {
            out.push(HotkeyEvent::Switch);
        }
        out
    }

    fn other_key(&mut self) -> Option<HotkeyEvent> {
        self.hold.other_key()
    }
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
    /// The STFU switch key's flag; zero means none.
    static SWITCH: AtomicU64 = AtomicU64::new(0);

    pub fn set_combo(combo: Vec<Key>) {
        let required = combo.into_iter().fold(CGEventFlags::empty(), |a, k| a | k);
        REQUIRED.store(required.bits(), AtomicOrdering::SeqCst);
    }

    pub fn set_switch(key: Option<Key>) {
        SWITCH.store(key.map_or(0, |k| k.bits()), AtomicOrdering::SeqCst);
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

                let keys = Arc::new(Mutex::new(Keys::new()));
                let needs_reenable = Arc::new(AtomicBool::new(false));
                let reenable_flag = needs_reenable.clone();

                // Modifier-flag changes press and release the hotkey. Key-down events only cancel
                // a held take: their flags can lack the Fn bit and would read as a false release.
                let tap = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
                    move |_proxy, etype, event| {
                        match etype {
                            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                                log::warn!("event tap disabled by macOS ({etype:?}); re-enabling");
                                reenable_flag.store(true, Ordering::SeqCst);
                                return CallbackResult::Keep;
                            }
                            _ => {}
                        }
                        if matches!(etype, CGEventType::KeyDown) {
                            if let Some(ev) = keys.lock().unwrap().other_key() {
                                log::info!("key pressed while the hotkey was held; take dropped");
                                let _ = tx.send(ev);
                            }
                            return CallbackResult::Keep;
                        }
                        let flags = event.get_flags();
                        let required =
                            CGEventFlags::from_bits_truncate(REQUIRED.load(AtomicOrdering::SeqCst));
                        let all_held = !required.is_empty() && flags.contains(required);
                        let switch =
                            CGEventFlags::from_bits_truncate(SWITCH.load(AtomicOrdering::SeqCst));
                        // A switch key inside the hotkey is held all along, so it cannot switch.
                        let switch_held = !switch.is_empty()
                            && !required.contains(switch)
                            && flags.contains(switch);
                        log::debug!(
                            "tap event {:?} keycode {} flags {:#x} held={all_held}",
                            etype,
                            event.get_integer_value_field(
                                core_graphics::event::EventField::KEYBOARD_EVENT_KEYCODE
                            ),
                            flags.bits()
                        );
                        for ev in keys.lock().unwrap().update(all_held, switch_held) {
                            let _ = tx.send(ev);
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
    /// The STFU switch key, if any.
    static SWITCH: Mutex<Option<Key>> = Mutex::new(None);

    pub fn set_combo(combo: Vec<Key>) {
        *COMBO.lock().unwrap() = combo;
    }

    pub fn set_switch(key: Option<Key>) {
        *SWITCH.lock().unwrap() = key;
    }

    fn is_modifier(k: Key) -> bool {
        use rdev::Key::*;
        matches!(
            k,
            ControlLeft | ControlRight | ShiftLeft | ShiftRight | Alt | AltGr | MetaLeft | MetaRight
                | Function | CapsLock
        )
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
                let mut keys = Keys::new();
                let callback = move |event: Event| {
                    let pressed = match event.event_type {
                        EventType::KeyPress(k) => {
                            held.insert(k);
                            Some(k)
                        }
                        EventType::KeyRelease(k) => {
                            held.remove(&k);
                            None
                        }
                        _ => return,
                    };
                    let (all_held, in_combo, switch_held) = {
                        let combo = COMBO.lock().unwrap();
                        let all = !combo.is_empty() && combo.iter().all(|k| held.contains(k));
                        // A switch key inside the hotkey is held all along, so it cannot switch.
                        let switch = SWITCH.lock().unwrap().filter(|k| !combo.contains(k));
                        let switch_held = switch.is_some_and(|k| held.contains(&k));
                        (all, pressed.is_some_and(|k| combo.contains(&k)), switch_held)
                    };
                    // Any other non-modifier key while the hotkey is held drops the take.
                    if let Some(k) = pressed {
                        if !in_combo && !is_modifier(k) {
                            if let Some(ev) = keys.other_key() {
                                log::info!("key pressed while the hotkey was held; take dropped");
                                let _ = tx.send(ev);
                            }
                            return;
                        }
                    }
                    // Windows repeats a held key's press; `Keys` only reacts to the switch key
                    // going from up to down, so the repeats do nothing.
                    for ev in keys.update(all_held, switch_held) {
                        let _ = tx.send(ev);
                    }
                };
                if let Err(e) = rdev::listen(callback) {
                    log::error!("global key listener failed: {e:?}");
                }
            })
            .expect("spawn hotkey thread");
    }
}

pub use imp::{parse_key, set_combo, set_switch, spawn, Key};

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
    fn a_lone_modifier_other_than_fn_is_refused() {
        for c in CHOICES {
            let one = [c.name.to_string()];
            assert_eq!(validate(&one).is_ok(), c.name == "Function", "{} alone", c.name);
        }
    }

    #[test]
    fn another_key_cancels_a_held_take_until_the_keys_are_let_go() {
        let mut h = Hold::Idle;
        assert_eq!(h.keys(true), Some(HotkeyEvent::Down));
        assert_eq!(h.other_key(), Some(HotkeyEvent::Cancel), "Fn+Shift+Left");
        assert_eq!(h.other_key(), None);
        assert_eq!(h.keys(true), None, "still held: no new take");
        assert_eq!(h.keys(false), None, "let go: nothing to stop");
        assert_eq!(h.keys(true), Some(HotkeyEvent::Down), "next press works");
        assert_eq!(h.keys(false), Some(HotkeyEvent::Up));
        assert_eq!(Hold::Idle.other_key(), None, "typing without the hotkey does nothing");
    }

    #[test]
    fn the_switch_key_flips_a_take_on_each_press() {
        use HotkeyEvent::*;
        let mut k = Keys::new();
        assert_eq!(k.update(false, true), vec![], "switch key alone does nothing");
        assert_eq!(k.update(false, false), vec![]);
        assert_eq!(k.update(true, false), vec![Down]);
        assert_eq!(k.update(true, true), vec![Switch]);
        assert_eq!(k.update(true, true), vec![], "a held or repeated key is one press");
        assert_eq!(k.update(true, false), vec![], "letting go keeps the switch");
        assert_eq!(k.update(true, true), vec![Switch], "a second press switches back");
        assert_eq!(k.update(false, true), vec![Up]);
        assert_eq!(k.update(false, false), vec![]);
    }

    #[test]
    fn a_switch_key_pressed_before_the_hotkey_counts() {
        use HotkeyEvent::*;
        let mut k = Keys::new();
        assert_eq!(k.update(false, true), vec![]);
        assert_eq!(k.update(true, true), vec![Down, Switch]);
        assert_eq!(k.update(false, false), vec![Up]);
        assert_eq!(k.update(true, false), vec![Down], "the next take starts plain");
    }

    #[test]
    fn a_cancelled_take_does_not_switch() {
        let mut k = Keys::new();
        assert_eq!(k.update(true, false), vec![HotkeyEvent::Down]);
        assert_eq!(k.other_key(), Some(HotkeyEvent::Cancel));
        assert_eq!(k.update(true, true), vec![]);
    }

    #[test]
    fn the_switch_key_must_not_be_part_of_the_hotkey() {
        let hotkey: Vec<String> = CHOICES[..2].iter().map(|c| c.name.to_string()).collect();
        let inside = CHOICES[0].name;
        let outside = CHOICES[3].name;
        assert!(validate_switch(inside, &hotkey).is_err());
        assert!(switch_key(inside, &hotkey).is_none());
        assert!(validate_switch(outside, &hotkey).is_ok());
        assert!(switch_key(outside, &hotkey).is_some());
        assert!(validate_switch("", &hotkey).is_ok(), "no switch key is allowed");
        assert!(validate_switch("Nope", &hotkey).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn switch_key_aliases_count_as_the_hotkey_key() {
        let hotkey = vec!["Function".to_string(), "CommandRight".to_string()];
        assert!(validate_switch("Command", &hotkey).is_err());
        assert!(validate_switch("CommandLeft", &hotkey).is_err());
        assert!(validate_switch("Option", &hotkey).is_ok());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn switch_key_aliases_count_as_the_hotkey_key() {
        let hotkey = vec!["ControlLeft".to_string(), "WinRight".to_string()];
        assert!(validate_switch("MetaRight", &hotkey).is_err());
        assert!(validate_switch("CommandRight", &hotkey).is_err());
        let default = vec!["ControlLeft".to_string(), "MetaLeft".to_string()];
        assert!(validate_switch("Alt", &default).is_ok(), "the default switch key works with the default hotkey");
        assert!(validate_switch("AltGr", &default).is_err(), "AltGr fakes a Ctrl press on Windows");
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
