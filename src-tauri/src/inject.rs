//! Puts text where the cursor is: clipboard + synthetic paste keystroke, then restores the clipboard.

use anyhow::{Context, Result};
use std::thread::sleep;
use std::time::Duration;

pub fn paste(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("opening clipboard")?;
    let previous = clipboard.get_text().ok();
    clipboard.set_text(text.to_string()).context("writing clipboard")?;
    sleep(Duration::from_millis(60));

    send_paste_keystroke()?;

    // Give the target app time to read the clipboard before we put the old content back.
    sleep(Duration::from_millis(250));
    if let Some(prev) = previous {
        let _ = clipboard.set_text(prev);
    }
    Ok(())
}

/// macOS: post Cmd+V as raw CGEvents using the virtual keycode for V. This deliberately avoids
/// any character-to-keycode lookup, because Text Input Services must not be called off the
/// main thread on macOS 26 and that is exactly what generic input libraries do.
#[cfg(target_os = "macos")]
fn send_paste_keystroke() -> Result<()> {
    use anyhow::anyhow;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const KVK_ANSI_V: u16 = 9;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow!("creating event source"))?;
    for down in [true, false] {
        let ev = CGEvent::new_keyboard_event(source.clone(), KVK_ANSI_V, down)
            .map_err(|_| anyhow!("creating key event"))?;
        ev.set_flags(CGEventFlags::CGEventFlagCommand);
        ev.post(CGEventTapLocation::HID);
        sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn send_paste_keystroke() -> Result<()> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default()).context("initialising input synthesis")?;
    enigo.key(Key::Control, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(Key::Control, Direction::Release)?;
    Ok(())
}
