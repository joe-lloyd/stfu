//! Local dictation history: one JSON Lines file per day, one line per dictation, plus the audio of
//! each take. Nothing here touches the network.
//!
//! Layout, next to config.json:
//!   history/2026-09-28.jsonl                       every stage of every dictation that day
//!   history/audio/2026-09-28/20260928-143012-123.wav  what the microphone heard
//!
//! Recordings from past days may be compressed to `<id>.flac` (lossless, about half the size)
//! when history nears its size limit; `read_audio` reads either.
//!
//! JSON Lines because it is append-only (a crash mid-write loses at most the line being written,
//! never the file), readable with any editor, and greppable or `jq`-able without the app.

use anyhow::{anyhow, bail, Context, Result};
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
    /// Path relative to the history folder; absent when audio is not kept. Always the `.wav`
    /// name it was saved under, even after the file is compressed to `.flac` beside it.
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
    /// Whether a text field had focus at paste time: `text_field`, `no_text_field` or `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// The text was left on the clipboard instead of putting the previous contents back.
    #[serde(default)]
    pub on_clipboard: bool,
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

/// A take's audio as WAV, whether it is stored as WAV or was compressed to FLAC since.
/// The most recent dictation that produced text, from the newest few days: (text, ts, app).
pub fn last_output(root: &Path) -> Option<(String, String, Option<String>)> {
    for day in days(root).ok()?.iter().take(3) {
        for r in read_day(root, &day.day).ok()? {
            let text = r["output"]["text"].as_str().unwrap_or_default();
            if !text.trim().is_empty() {
                let app = r["target_app"]["name"].as_str().map(String::from);
                return Some((text.to_string(), r["ts"].as_str().unwrap_or_default().to_string(), app));
            }
        }
    }
    None
}

pub fn read_audio(root: &Path, id: &str) -> Result<Vec<u8>> {
    check_id(id)?;
    let folder = root.join("audio").join(day_of(id)?);
    let wav = folder.join(format!("{id}.wav"));
    match std::fs::read(&wav) {
        Ok(bytes) => return Ok(bytes),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("reading {}", wav.display()))
        }
        Err(_) => {}
    }
    let flac = folder.join(format!("{id}.flac"));
    let bytes = std::fs::read(&flac)
        .with_context(|| format!("no audio kept for this dictation ({})", wav.display()))?;
    crate::flac::decode_to_wav(&bytes).with_context(|| format!("decoding {}", flac.display()))
}

/// Compresses every WAV recording of one day to FLAC. Each file is decoded again and compared
/// sample by sample before its WAV is removed, so a fault here can cost space but never audio.
/// Returns the bytes before and after.
pub fn compress_day(root: &Path, day: &str) -> Result<(u64, u64)> {
    check_day(day)?;
    let folder = root.join("audio").join(day);
    let Ok(entries) = std::fs::read_dir(&folder) else { return Ok((0, 0)) };
    let (mut before, mut after) = (0u64, 0u64);
    for e in entries.filter_map(|e| e.ok()) {
        let path = e.path();
        let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".wav")) else {
            continue;
        };
        if check_id(id).is_err() {
            continue;
        }
        let wav = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let pcm = match crate::flac::parse_wav(&wav) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("not compressing {}: {e:#}", path.display());
                continue;
            }
        };
        let flac = crate::flac::encode(&pcm)?;
        if crate::flac::decode(&flac).ok().as_ref() != Some(&pcm) {
            bail!("compressed {} did not match the original; left it as it was", path.display());
        }
        let part = folder.join(format!("{id}.flac.part"));
        std::fs::write(&part, &flac).with_context(|| format!("writing {}", part.display()))?;
        std::fs::rename(&part, folder.join(format!("{id}.flac")))?;
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        before += wav.len() as u64;
        after += flac.len() as u64;
    }
    Ok((before, after))
}

/// Past days that still hold uncompressed recordings. Today's are left alone: they are the
/// ones most likely to be replayed, and the folder is still being written to.
pub fn compressible_days(root: &Path, today: &str) -> Vec<String> {
    day_sizes(root).into_iter().filter(|d| d.wav > 0 && d.day != today).map(|d| d.day).collect()
}

/// Compression runs on its own once history passes the warning share of its size limit. It
/// loses nothing, so unlike deleting it never waits for a yes.
pub fn compression_due(root: &Path, limits: Limits, today: &str) -> bool {
    limits.max_bytes > 0
        && usage(root, today).total_bytes as f64 >= limits.max_bytes as f64 * WARN_AT
        && !compressible_days(root, today).is_empty()
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

/// Retention limits, from config. Zero means "no limit" for either.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub keep_days: u32,
    pub max_bytes: u64,
}

/// Warn once usage passes this share of the size limit, before anything needs deleting.
pub const WARN_AT: f64 = 0.8;

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Usage {
    pub total_bytes: u64,
    pub audio_bytes: u64,
    pub text_bytes: u64,
    pub days: usize,
    pub oldest_day: Option<String>,
    /// Past days' recordings still stored as WAV, which compression would roughly halve.
    pub compressible_bytes: u64,
}

/// What a cleanup would delete. Built without touching anything, so it can be shown to the user
/// and confirmed before it runs.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Plan {
    /// Whole days past `keep_days`: transcript, result and audio all go.
    pub expired_days: Vec<String>,
    /// Days whose audio goes to get back under the size limit; their text stays.
    pub audio_days: Vec<String>,
    pub frees_bytes: u64,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.expired_days.is_empty() && self.audio_days.is_empty()
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries
        .filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

struct DaySize {
    day: String,
    text: u64,
    /// Every recording of the day, WAV and FLAC.
    audio: u64,
    /// The part of `audio` that is still WAV.
    wav: u64,
}

/// Per-day sizes, oldest first.
fn day_sizes(root: &Path) -> Vec<DaySize> {
    let mut map = std::collections::BTreeMap::<String, (u64, u64, u64)>::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.filter_map(|e| e.ok()) {
            let Ok(name) = e.file_name().into_string() else { continue };
            let Some(day) = name.strip_suffix(".jsonl") else { continue };
            if check_day(day).is_ok() {
                map.entry(day.to_string()).or_default().0 = e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(root.join("audio")) {
        for e in entries.filter_map(|e| e.ok()) {
            let Ok(day) = e.file_name().into_string() else { continue };
            if check_day(&day).is_ok() {
                let wav = std::fs::read_dir(e.path())
                    .map(|files| {
                        files
                            .filter_map(|f| f.ok())
                            .filter(|f| f.file_name().to_str().is_some_and(|n| n.ends_with(".wav")))
                            .filter_map(|f| f.metadata().ok())
                            .map(|m| m.len())
                            .sum()
                    })
                    .unwrap_or(0);
                let entry = map.entry(day).or_default();
                entry.1 = dir_size(&e.path());
                entry.2 = wav;
            }
        }
    }
    map.into_iter().map(|(day, (text, audio, wav))| DaySize { day, text, audio, wav }).collect()
}

pub fn usage(root: &Path, today: &str) -> Usage {
    let sizes = day_sizes(root);
    let text_bytes = sizes.iter().map(|s| s.text).sum();
    let audio_bytes = sizes.iter().map(|s| s.audio).sum();
    Usage {
        total_bytes: text_bytes + audio_bytes,
        audio_bytes,
        text_bytes,
        days: sizes.len(),
        oldest_day: sizes.first().map(|s| s.day.clone()),
        compressible_bytes: sizes.iter().filter(|s| s.day != today).map(|s| s.wav).sum(),
    }
}

/// Decides what to delete. Age first: days older than `keep_days` go entirely. Then size: if
/// still over `max_bytes`, drop the oldest days' audio, because audio is nearly all of the space
/// and a transcript costs a couple of KB. Only if text alone is over the limit do the oldest
/// whole days go too. Today is never touched.
pub fn plan(root: &Path, limits: Limits, today: &str) -> Plan {
    let sizes = day_sizes(root);
    let mut plan = Plan::default();
    let cutoff = (limits.keep_days > 0)
        .then(|| chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok())
        .flatten()
        .map(|t| (t - chrono::Duration::days(limits.keep_days as i64)).format("%Y-%m-%d").to_string());

    let mut remaining = Vec::new();
    for DaySize { day, text, audio, .. } in sizes {
        if cutoff.as_deref().is_some_and(|c| day.as_str() < c) && day != today {
            plan.frees_bytes += text + audio;
            plan.expired_days.push(day);
        } else {
            remaining.push((day, text, audio));
        }
    }

    if limits.max_bytes > 0 {
        let mut total: u64 = remaining.iter().map(|s| s.1 + s.2).sum();
        for (day, _, audio) in remaining.iter() {
            if total <= limits.max_bytes {
                break;
            }
            if *audio > 0 && day != today {
                total -= audio;
                plan.frees_bytes += audio;
                plan.audio_days.push(day.clone());
            }
        }
        for (day, text, audio) in remaining.iter() {
            if total <= limits.max_bytes {
                break;
            }
            if day != today && !plan.expired_days.contains(day) {
                let already = if plan.audio_days.contains(day) { 0 } else { *audio };
                total -= text + already;
                plan.frees_bytes += text + already;
                plan.audio_days.retain(|d| d != day);
                plan.expired_days.push(day.clone());
            }
        }
    }
    plan
}

/// Carries out a plan. Everything it names is re-validated as a plain `YYYY-MM-DD`.
pub fn apply(root: &Path, plan: &Plan) -> Result<()> {
    for day in &plan.expired_days {
        delete_day(root, day)?;
    }
    for day in &plan.audio_days {
        check_day(day)?;
        let audio = root.join("audio").join(day);
        if audio.exists() {
            std::fs::remove_dir_all(&audio).with_context(|| format!("deleting {}", audio.display()))?;
        }
    }
    if !plan.is_empty() {
        log::info!(
            "history cleanup: {} day(s) removed, audio removed from {} day(s), {} KB freed",
            plan.expired_days.len(),
            plan.audio_days.len(),
            plan.frees_bytes / 1024
        );
    }
    Ok(())
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
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

    /// Writes a day with `text` bytes of history and `audio` bytes of recordings.
    fn seed(root: &Path, day: &str, text: usize, audio: usize) {
        create_private_dir(root).unwrap();
        std::fs::write(root.join(format!("{day}.jsonl")), vec![b'x'; text]).unwrap();
        if audio > 0 {
            let dir = root.join("audio").join(day);
            create_private_dir(&dir).unwrap();
            std::fs::write(dir.join("a.wav"), vec![0u8; audio]).unwrap();
        }
    }

    #[test]
    fn days_past_the_age_limit_go_entirely() {
        let root = temp_root("age");
        seed(&root, "2026-08-01", 10, 100);
        seed(&root, "2026-09-20", 10, 100);
        seed(&root, "2026-09-28", 10, 100);
        let p = plan(&root, Limits { keep_days: 30, max_bytes: 0 }, "2026-09-28");
        assert_eq!(p.expired_days, vec!["2026-08-01"]);
        assert!(p.audio_days.is_empty());
        assert_eq!(p.frees_bytes, 110);

        apply(&root, &p).unwrap();
        assert_eq!(usage(&root, "2026-09-28").days, 2);
        assert_eq!(usage(&root, "2026-09-28").oldest_day.as_deref(), Some("2026-09-20"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn over_the_size_limit_the_oldest_audio_goes_first_and_text_stays() {
        let root = temp_root("size");
        seed(&root, "2026-09-26", 10, 500);
        seed(&root, "2026-09-27", 10, 500);
        seed(&root, "2026-09-28", 10, 500);
        let p = plan(&root, Limits { keep_days: 0, max_bytes: 1100 }, "2026-09-28");
        assert!(p.expired_days.is_empty(), "transcripts are kept");
        assert_eq!(p.audio_days, vec!["2026-09-26"]);
        assert_eq!(p.frees_bytes, 500);

        apply(&root, &p).unwrap();
        let u = usage(&root, "2026-09-28");
        assert_eq!(u.days, 3);
        assert_eq!(u.audio_bytes, 1000);
        assert!(plan(&root, Limits { keep_days: 0, max_bytes: 1100 }, "2026-09-28").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn today_is_never_deleted_and_no_limits_means_nothing_to_do() {
        let root = temp_root("today");
        seed(&root, "2026-09-28", 10, 5000);
        assert!(plan(&root, Limits { keep_days: 1, max_bytes: 100 }, "2026-09-28").is_empty());
        seed(&root, "2020-01-01", 10, 5000);
        assert!(plan(&root, Limits { keep_days: 0, max_bytes: 0 }, "2026-09-28").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A real, compressible recording: a tone with some noise, like speech in a quiet room.
    fn tone_wav(frames: usize) -> Vec<u8> {
        let samples = (0..frames)
            .map(|i| ((i as f64 * 0.07).sin() * 9000.0 + ((i * 7919) % 61) as f64 - 30.0) as i16)
            .collect();
        crate::flac::to_wav(&crate::flac::Pcm { rate: 16000, channels: 1, samples })
    }

    #[test]
    fn near_the_size_limit_past_days_are_compressed_losslessly() {
        let root = temp_root("compress");
        let (old, today) = ("20260927-101010-000", "20260928-090000-000");
        let old_wav = tone_wav(64_000);
        save_audio(&root, old, &old_wav).unwrap();
        save_audio(&root, today, &tone_wav(64_000)).unwrap();
        // A leftover that is not audio stays where it is and does not stop the rest.
        std::fs::write(root.join("audio/2026-09-27/20260927-111111-000.wav"), b"junk").unwrap();

        let before = usage(&root, "2026-09-28");
        assert_eq!(before.compressible_bytes, old_wav.len() as u64 + 4, "today is not counted");
        let roomy = Limits { keep_days: 0, max_bytes: before.total_bytes * 2 };
        assert!(!compression_due(&root, roomy, "2026-09-28"), "well under the limit");
        let tight = Limits { keep_days: 0, max_bytes: before.total_bytes };
        assert!(compression_due(&root, tight, "2026-09-28"));
        assert_eq!(compressible_days(&root, "2026-09-28"), vec!["2026-09-27"]);

        let (was, now) = compress_day(&root, "2026-09-27").unwrap();
        assert_eq!(was, old_wav.len() as u64);
        assert!(now < was / 2, "{now} of {was}");
        assert!(!root.join("audio/2026-09-27/20260927-101010-000.wav").exists());
        assert!(root.join("audio/2026-09-27/20260927-101010-000.flac").exists());
        assert_eq!(read_audio(&root, old).unwrap(), old_wav, "plays back bit for bit");
        assert!(root.join("audio/2026-09-28/20260928-090000-000.wav").exists());

        let after = usage(&root, "2026-09-28");
        assert_eq!(after.total_bytes, before.total_bytes - was + now);
        assert_eq!(after.compressible_bytes, 4, "only the unreadable file is left");
        // The size limit then plans deletions from the compressed sizes.
        assert!(plan(&root, Limits { keep_days: 0, max_bytes: after.total_bytes }, "2026-09-28").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wav_duration_comes_from_the_header() {
        let wav = crate::audio::silent_wav(1.5).unwrap();
        assert_eq!(wav_duration_ms(&wav), 1500);
        assert_eq!(wav_duration_ms(b"short"), 0);
    }
}
