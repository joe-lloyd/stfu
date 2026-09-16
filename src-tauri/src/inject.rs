//! Puts text where the cursor is: clipboard + synthetic paste, then restores the clipboard.

use anyhow::{Context, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread::sleep;
use std::time::Duration;

pub fn paste(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("opening clipboard")?;
    let previous = clipboard.get_text().ok();
    clipboard.set_text(text.to_string()).context("writing clipboard")?;
    sleep(Duration::from_millis(60));

    let mut enigo = Enigo::new(&Settings::default()).context("initialising input synthesis")?;
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    enigo.key(modifier, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(modifier, Direction::Release)?;

    // Give the target app time to read the clipboard before we put the old content back.
    sleep(Duration::from_millis(250));
    if let Some(prev) = previous {
        let _ = clipboard.set_text(prev);
    }
    Ok(())
}
