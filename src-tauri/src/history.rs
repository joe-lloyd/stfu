//! Local dictation history: one JSON Lines file per day, one line per dictation, plus the audio of
//! each take. Nothing here touches the network.
//!
//! Layout, next to config.json:
//!   history/2026-09-28.jsonl                       every stage of every dictation that day
//!   history/audio/2026-09-28/20260928-143012-123.wav  what the microphone heard
//!
//! JSON Lines because it is append-only (a crash mid-write loses at most the line being written,
//! never the file), readable with any editor, and greppable or `jq`-able without the app.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Bumped when a field changes meaning, so old files can still be read correctly.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub v: u32,
    /// Unique per take and sortable: `YYYYMMDD-HHMMSS-mmm`. Also the audio file's name.
    pub id: String,
    /// Local time the hotkey went down, RFC 3339 with offset.
    pub ts: String,
    pub app_version: String,
    pub profile: String,
    pub language: String,
    /// The app that had focus when recording started, i.e. where the text was pasted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_app: Option<TargetApp>,
    /// `pasted`, `no_speech`, `empty_cleanup` or `failed`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub audio: Audio,
    pub stt: Stt,
    pub llm: Llm,
    pub output: Output,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetApp {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Audio {
    /// Path relative to the history folder; absent when audio is not kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// How long the hotkey was held.
    pub held_ms: u64,
    /// Length of the recorded audio.
    pub duration_ms: u64,
    pub bytes: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stt {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    pub model: String,
    pub duration_ms: u64,
    /// The raw transcript, exactly as the speech-to-text server returned it (trimmed).
    pub text: String,
    /// The server's whole JSON reply, which can carry request ids and other provider extras.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Llm {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<String>,
    pub duration_ms: u64,
    /// The prompt's user message: the transcript plus any language hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The model's reply before anything was extracted from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    /// The cleaned text the model produced, if it produced a usable one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaned: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// True when clean-up failed and the raw transcript was pasted instead.
    pub fell_back: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Output {
    /// The text that was (or would have been) pasted.
    pub text: String,
    pub pasted: bool,
    pub paste_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste_error: Option<String>,
}

pub fn dir() -> Result<PathBuf> {
    Ok(crate::config::Config::path()?.with_file_name("history"))
}

/// A new record stamped with the current local time.
pub fn new_record() -> Record {
    let now = chrono::Local::now();
    Record {
        v: SCHEMA,
        id: now.format("%Y%m%d-%H%M%S-%3f").to_string(),
        ts: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        ..Default::default()
    }
}

/// `YYYYMMDD-...` -> `YYYY-MM-DD`, the day file a record belongs to.
fn day_of(id: &str) -> Result<String> {
    let d = id.get(0..8).filter(|d| d.bytes().all(|b| b.is_ascii_digit()));
    let d = d.ok_or_else(|| anyhow!("bad record id {id:?}"))?;
    Ok(format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8]))
}

/// Guards every path built from webview input: only `YYYY-MM-DD` gets through.
fn check_day(day: &str) -> Result<()> {
    let ok = day.len() == 10
        && day.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
    if ok { Ok(()) } else { Err(anyhow!("bad day {day:?}")) }
}

/// Same for record ids: digits and dashes only, so they can never escape the audio folder.
fn check_id(id: &str) -> Result<()> {
    if !id.is_empty() && id.len() < 40 && id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        Ok(())
    } else {
        Err(anyhow!("bad record id {id:?}"))
    }
}

/// Dictations can be private, so keep the folder readable by this user only.
fn create_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Stores the take's audio and returns its path relative to the history folder.
pub fn save_audio(root: &Path, id: &str, wav: &[u8]) -> Result<String> {
    check_id(id)?;
    let day = day_of(id)?;
    let folder = root.join("audio").join(&day);
    create_private_dir(&folder)?;
    let path = folder.join(format!("{id}.wav"));
    std::fs::write(&path, wav).with_context(|| format!("writing {}", path.display()))?;
    Ok(format!("audio/{day}/{id}.wav"))
}

/// Appends one record as one line to that day's file.
pub fn append(root: &Path, record: &Record) -> Result<()> {
    let day = day_of(&record.id)?;
    create_private_dir(root)?;
    let path = root.join(format!("{day}.jsonl"));
    let mut line = serde_json::to_string(record)?;
    line.push('\n');
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&path).with_context(|| format!("opening {}", path.display()))?;
    // One write call per line, so a line is never interleaved or half-merged with another.
    file.write_all(line.as_bytes()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Day {
    pub day: String,
    pub count: usize,
}

/// Every day that has a history file, newest first.
pub fn days(root: &Path) -> Result<Vec<Day>> {
    let Ok(entries) = std::fs::read_dir(root) else { return Ok(vec![]) };
    let mut out: Vec<Day> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let day = name.strip_suffix(".jsonl")?.to_string();
            check_day(&day).ok()?;
            let count = std::fs::read_to_string(e.path())
                .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
                .unwrap_or(0);
            Some(Day { day, count })
        })
        .collect();
    out.sort_by(|a, b| b.day.cmp(&a.day));
    Ok(out)
}

/// One day's records, newest first. Returned as raw JSON so lines written by a newer or older
/// version still show, and a damaged line is skipped instead of hiding the whole day.
pub fn read_day(root: &Path, day: &str) -> Result<Vec<serde_json::Value>> {
    check_day(day)?;
    let path = root.join(format!("{day}.jsonl"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut out: Vec<serde_json::Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match serde_json::from_str(l) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("skipping unreadable history line in {day}: {e}");
                None
            }
        })
        .collect();
    out.reverse();
    Ok(out)
}

pub fn read_audio(root: &Path, id: &str) -> Result<Vec<u8>> {
    check_id(id)?;
    let path = root.join("audio").join(day_of(id)?).join(format!("{id}.wav"));
    std::fs::read(&path).with_context(|| format!("no audio kept for this dictation ({})", path.display()))
}

/// Removes a day's history file and its audio folder.
pub fn delete_day(root: &Path, day: &str) -> Result<()> {
    check_day(day)?;
    let file = root.join(format!("{day}.jsonl"));
    if file.exists() {
        std::fs::remove_file(&file).with_context(|| format!("deleting {}", file.display()))?;
    }
    let audio = root.join("audio").join(day);
    if audio.exists() {
        std::fs::remove_dir_all(&audio).with_context(|| format!("deleting {}", audio.display()))?;
    }
    Ok(())
}

/// Length of a 16-bit PCM WAV from its header.
pub fn wav_duration_ms(wav: &[u8]) -> u64 {
    if wav.len() < 44 {
        return 0;
    }
    let u16_at = |i: usize| u16::from_le_bytes([wav[i], wav[i + 1]]) as u64;
    let u32_at = |i: usize| u32::from_le_bytes([wav[i], wav[i + 1], wav[i + 2], wav[i + 3]]) as u64;
    let (channels, rate, bits) = (u16_at(22), u32_at(24), u16_at(34));
    let bytes_per_sec = channels * rate * bits / 8;
    if bytes_per_sec == 0 {
        return 0;
    }
    (wav.len() as u64 - 44) * 1000 / bytes_per_sec
}

/// The app that has focus right now. macOS only; elsewhere this is not captured yet.
#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<TargetApp> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    objc2::rc::autoreleasepool(|_| unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![&*ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let text = |s: *mut NSString| (!s.is_null()).then(|| (*s).to_string());
        let name: *mut NSString = msg_send![&*app, localizedName];
        let bundle: *mut NSString = msg_send![&*app, bundleIdentifier];
        Some(TargetApp { name: text(name).unwrap_or_default(), bundle_id: text(bundle) })
    })
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<TargetApp> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stfu-history-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn day_and_id_guards_reject_path_tricks() {
        assert!(check_day("2026-09-28").is_ok());
        assert!(check_day("../../etc").is_err());
        assert!(check_day("2026-09-2x").is_err());
        assert!(check_id("20260928-143012-123").is_ok());
        assert!(check_id("../20260928").is_err());
        assert!(check_id("").is_err());
        assert_eq!(day_of("20260928-143012-123").unwrap(), "2026-09-28");
    }

    #[test]
    fn records_round_trip_through_the_day_file_newest_first() {
        let root = temp_root("roundtrip");
        let mut a = new_record();
        a.id = "20260928-090000-000".into();
        a.stt.text = "um hello there".into();
        a.output.text = "Hello there.".into();
        let mut b = a.clone();
        b.id = "20260928-100000-000".into();
        b.output.text = "Second.".into();
        append(&root, &a).unwrap();
        append(&root, &b).unwrap();
        // A damaged line must not hide the rest of the day.
        std::fs::OpenOptions::new().append(true).open(root.join("2026-09-28.jsonl")).unwrap()
            .write_all(b"{not json\n").unwrap();

        let rows = read_day(&root, "2026-09-28").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["output"]["text"], "Second.");
        assert_eq!(rows[1]["stt"]["text"], "um hello there");
        assert_eq!(days(&root).unwrap(), vec![Day { day: "2026-09-28".into(), count: 3 }]);

        let rel = save_audio(&root, &a.id, b"RIFF....").unwrap();
        assert_eq!(rel, "audio/2026-09-28/20260928-090000-000.wav");
        assert_eq!(read_audio(&root, &a.id).unwrap(), b"RIFF....");

        delete_day(&root, "2026-09-28").unwrap();
        assert!(days(&root).unwrap().is_empty());
        assert!(read_audio(&root, &a.id).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn frontmost_app_is_readable() {
        // Whatever is in front while tests run (a terminal, usually) comes back with a name. A
        // headless CI runner may have nothing in front; that must be None, not a crash.
        if let Some(app) = frontmost_app() {
            assert!(!app.name.is_empty());
        }
    }

    #[test]
    fn wav_duration_comes_from_the_header() {
        let wav = crate::audio::silent_wav(1.5).unwrap();
        assert_eq!(wav_duration_ms(&wav), 1500);
        assert_eq!(wav_duration_ms(b"short"), 0);
    }
}
