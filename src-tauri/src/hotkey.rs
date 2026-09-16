use anyhow::{anyhow, Result};
use rdev::{Event, EventType, Key};
use std::collections::HashSet;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

pub fn parse_key(name: &str) -> Result<Key> {
    Ok(match name {
        "Function" | "Fn" => Key::Function,
        "ControlLeft" => Key::ControlLeft,
        "ControlRight" => Key::ControlRight,
        "MetaLeft" | "CommandLeft" | "WinLeft" => Key::MetaLeft,
        "MetaRight" | "CommandRight" | "WinRight" => Key::MetaRight,
        "Alt" | "AltLeft" | "OptionLeft" => Key::Alt,
        "AltGr" | "AltRight" | "OptionRight" => Key::AltGr,
        "ShiftLeft" => Key::ShiftLeft,
        "ShiftRight" => Key::ShiftRight,
        "CapsLock" => Key::CapsLock,
        other => return Err(anyhow!("unsupported hotkey name: {other}")),
    })
}

/// Spawns the global key listener. Emits `Down` once when every key in `combo` is held,
/// and `Up` when any of them is released. Blocks its own thread forever.
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
                log::error!(
                    "global key listener failed: {e:?}. On macOS grant Accessibility and Input Monitoring \
                     to this app (or to your terminal when running in dev)."
                );
            }
        })
        .expect("spawn hotkey thread");
}
