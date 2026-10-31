//! Meeting transcripts: the microphone ("You") and the call apps' audio ("Others", see syscap.rs)
//! are cut into chunks of about 30 seconds and transcribed on this machine with a small local
//! model. Each chunk's text is appended to a plain text file as soon as it is ready. No audio is
//! ever written to disk and nothing leaves the machine; the transcript is meant to be handed to
//! another tool, not reread.

use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::audio::{self, TARGET_RATE};
use crate::syscap;

const RATE: usize = TARGET_RATE as usize;
/// Longest chunk sent to the model.
const CHUNK: usize = 30 * RATE;
/// A chunk ends at the quietest moment of its last few seconds, so words are rarely cut in two.
const SEARCH: usize = 6 * RATE;
const FRAME: usize = RATE / 10;
/// Below this RMS a chunk is silence (or a muted mic) and is not transcribed.
const SILENCE_RMS: f32 = 0.001;

/// The speech model: NVIDIA Parakeet TDT-CTC 110M, English, in transcribe.cpp's GGUF format.
/// Measured on an Intel i9 MacBook Pro, CPU only: 11-14x faster than real time.
pub struct ModelFile {
    pub name: &'static str,
    file: &'static str,
    url: &'static str,
    sha256: &'static str,
    pub bytes: u64,
}

pub const MODEL: ModelFile = ModelFile {
    name: "Parakeet 110M (English)",
    file: "parakeet-tdt_ctc-110m-Q8_0.gguf",
    // Pinned to a commit so the checksum below cannot go stale.
    url: "https://huggingface.co/handy-computer/parakeet-tdt_ctc-110m-gguf/resolve/1c3f0d5378fdd358905cc410a93ed9e64f52d170/parakeet-tdt_ctc-110m-Q8_0.gguf",
    sha256: "7dd44c74a331d788a4e5f8b16913b3feb29ced22cf5613aad0e0f6cd30516296",
    bytes: 135_373_280,
};

fn models_dir() -> Result<PathBuf> {
    Ok(crate::config::Config::path()?.with_file_name("models"))
}

pub fn model_path() -> Result<PathBuf> {
    Ok(models_dir()?.join(MODEL.file))
}

pub fn model_ready() -> bool {
    model_path().map(|p| p.exists()).unwrap_or(false)
}

/// Where transcripts go, one text file per meeting.
pub fn dir() -> Result<PathBuf> {
    Ok(crate::config::Config::path()?.with_file_name("meetings"))
}

/// The newest transcript, for "Copy last meeting transcript".
pub fn last_transcript() -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir().ok()?)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    // Names start with the date and time, so the last one by name is the newest.
    files.sort();
    files.pop()
}

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

pub fn downloading() -> bool {
    DOWNLOADING.load(Ordering::SeqCst)
}

/// Downloads the model next to the config, checks it against its SHA-256, and only then moves
/// it into place, so a cut-off or corrupted download is never loaded.
pub async fn download_model(mut progress: impl FnMut(u64, u64)) -> Result<()> {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        bail!("the model is already downloading");
    }
    // Cleared however the download ends, including when its future is dropped.
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            DOWNLOADING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    download(&mut progress).await
}

async fn download(progress: &mut impl FnMut(u64, u64)) -> Result<()> {
    let dir = models_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let part = dir.join(format!("{}.part", MODEL.file));
    // No overall timeout, since a slow line may need minutes, but a stalled one fails.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()?;
    let mut resp = client
        .get(MODEL.url)
        .send()
        .await
        .context("downloading the speech model")?
        .error_for_status()
        .context("downloading the speech model")?;
    let total = resp.content_length().unwrap_or(MODEL.bytes);
    let mut file = std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
    let mut sha = Sha256::new();
    let mut done = 0u64;
    while let Some(bytes) = resp.chunk().await.context("downloading the speech model")? {
        file.write_all(&bytes).with_context(|| format!("writing {}", part.display()))?;
        sha.update(&bytes);
        done += bytes.len() as u64;
        progress(done, total);
    }
    file.sync_all()?;
    drop(file);
    let got = sha.finish();
    if got != MODEL.sha256 {
        let _ = std::fs::remove_file(&part);
        bail!("the download was corrupted (checksum {got}); try again");
    }
    std::fs::rename(&part, model_path()?).context("moving the model into place")?;
    log::info!("speech model downloaded and verified: {}", MODEL.file);
    Ok(())
}

pub fn delete_model() -> Result<()> {
    if running() {
        bail!("stop the meeting transcript first");
    }
    let path = model_path()?;
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).with_context(|| format!("deleting {}", path.display())),
        _ => Ok(()),
    }
}

/// SHA-256 from CommonCrypto, which ships with macOS.
#[repr(C)]
struct Sha256([u32; 26]); // CC_SHA256_CTX

extern "C" {
    fn CC_SHA256_Init(ctx: *mut Sha256) -> i32;
    fn CC_SHA256_Update(ctx: *mut Sha256, data: *const u8, len: u32) -> i32;
    fn CC_SHA256_Final(md: *mut u8, ctx: *mut Sha256) -> i32;
}

impl Sha256 {
    fn new() -> Self {
        let mut ctx = Sha256([0; 26]);
        unsafe { CC_SHA256_Init(&mut ctx) };
        ctx
    }

    fn update(&mut self, data: &[u8]) {
        for part in data.chunks(u32::MAX as usize) {
            unsafe { CC_SHA256_Update(self, part.as_ptr(), part.len() as u32) };
        }
    }

    fn finish(mut self) -> String {
        let mut md = [0u8; 32];
        unsafe { CC_SHA256_Final(md.as_mut_ptr(), &mut self) };
        md.iter().map(|b| format!("{b:02x}")).collect()
    }
}

enum Ctl {
    Stop(mpsc::Sender<Result<PathBuf>>),
}

/// True from the moment a meeting starts until its thread has ended, however it ends.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// How to reach the running meeting. Only ever locked briefly.
static MEETING: Mutex<Option<mpsc::Sender<Ctl>>> = Mutex::new(None);

pub fn running() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// Marks the meeting as over when dropped, so an error or a panic in the meeting thread can never
/// leave the app showing a meeting that is not being transcribed.
struct Active;

impl Drop for Active {
    fn drop(&mut self) {
        MEETING.lock().unwrap().take();
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Starts transcribing. Returns once the model is loaded and both sides are being captured, with
/// the path of the transcript file. If the meeting fails after that, `on_failure` gets the error.
pub fn start(apps: Vec<String>, on_failure: impl FnOnce(anyhow::Error) + Send + 'static) -> Result<PathBuf> {
    if !syscap::supported() {
        bail!("meeting transcripts need macOS 13 or newer");
    }
    let model = model_path()?;
    if !model.exists() {
        bail!("download the speech model in Settings first");
    }
    if ACTIVE.swap(true, Ordering::SeqCst) {
        bail!("a meeting transcript is already running");
    }
    let active = Active;
    let started = chrono::Local::now();
    let path = dir()?.join(format!("{}.txt", started.format("%Y-%m-%d_%H-%M-%S")));
    let (ctl_tx, ctl_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    *MEETING.lock().unwrap() = Some(ctl_tx);
    let thread_path = path.clone();
    std::thread::Builder::new()
        .name("meeting".into())
        .spawn(move || {
            let mut ready = Some(ready_tx);
            match run(&model, apps, &thread_path, started, ctl_rx, &mut ready) {
                Ok(reply) => {
                    // Over before `stop` hears back, so it can start the next one straight away.
                    drop(active);
                    if let Some(reply) = reply {
                        let _ = reply.send(Ok(thread_path));
                    }
                }
                Err(e) => {
                    drop(active);
                    log::error!("meeting transcript: {e:#}");
                    match ready {
                        Some(ready) => drop(ready.send(Err(e))),
                        None => on_failure(e),
                    }
                }
            }
        })
        .context("spawning the meeting thread")?;
    ready_rx.recv().map_err(|_| anyhow!("the meeting recorder stopped unexpectedly"))??;
    log::info!("meeting transcript started: {}", path.display());
    Ok(path)
}

/// Stops capturing, transcribes what is left, and returns the finished transcript's path.
pub fn stop() -> Result<PathBuf> {
    let ctl = MEETING.lock().unwrap().clone().ok_or_else(|| anyhow!("no meeting transcript is running"))?;
    let (tx, rx) = mpsc::channel();
    ctl.send(Ctl::Stop(tx)).map_err(|_| anyhow!("the meeting recorder stopped unexpectedly"))?;
    rx.recv().map_err(|_| anyhow!("the meeting recorder stopped unexpectedly"))?
}

struct Track {
    label: &'static str,
    /// 16 kHz mono not yet sent to the model.
    pending: Vec<f32>,
    /// Samples already sent, for the timestamp of the next chunk.
    consumed: usize,
}

fn run(
    model_path: &Path,
    apps: Vec<String>,
    path: &Path,
    started: chrono::DateTime<chrono::Local>,
    ctl: mpsc::Receiver<Ctl>,
    ready: &mut Option<mpsc::Sender<Result<()>>>,
) -> Result<Option<mpsc::Sender<Result<PathBuf>>>> {
    transcribe_cpp::init_logging();
    let model = transcribe_cpp::Model::load(model_path).context("loading the speech model")?;
    let mut session = new_session(&model)?;
    let mut out = Transcript::create(path, started)?;

    // The microphone delivers its own rate and channel count; it is converted on each tick.
    let mic_raw = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = mic_raw.clone();
    let (mic, mic_rate, mic_channels) =
        audio::open_input(move |data| sink.lock().unwrap().extend_from_slice(data))?;
    let others = Arc::new(Mutex::new(Vec::<f32>::new()));
    let call = syscap::AppAudio::start(apps, others.clone())?;
    let mut mic = Some(mic);
    let mut call = Some(call);
    if let Some(ready) = ready.take() {
        let _ = ready.send(Ok(()));
    }

    let mut you = Track { label: "You", pending: Vec::new(), consumed: 0 };
    let mut them = Track { label: "Others", pending: Vec::new(), consumed: 0 };
    loop {
        let reply = match ctl.recv_timeout(Duration::from_millis(500)) {
            Ok(Ctl::Stop(reply)) => Some(Some(reply)),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(None),
        };
        let last = reply.is_some();
        if last {
            // Stop capturing first so the final drain really is the end.
            drop(mic.take());
            drop(call.take());
        }
        let raw = std::mem::take(&mut *mic_raw.lock().unwrap());
        you.pending.extend(audio::to_mono_16k(&raw, mic_rate, mic_channels));
        them.pending.append(&mut others.lock().unwrap());
        for track in [&mut you, &mut them] {
            transcribe_pending(&mut session, track, last, &mut out)?;
        }
        if let Some(reply) = reply {
            out.finish()?;
            log::info!("meeting transcript finished: {}", path.display());
            return Ok(reply);
        }
    }
}

/// A session on half the cores, so the call app itself stays smooth while a chunk is transcribed.
fn new_session(model: &transcribe_cpp::Model) -> Result<transcribe_cpp::Session> {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let options = transcribe_cpp::SessionOptions { n_threads: (cores / 2).max(2) as i32, ..Default::default() };
    model.session_with(&options).context("starting the speech model")
}

/// Transcribes every full chunk waiting on `track` (and the remainder when `flush`) into `out`.
fn transcribe_pending(
    session: &mut transcribe_cpp::Session,
    track: &mut Track,
    flush: bool,
    out: &mut Transcript,
) -> Result<()> {
    while let Some(chunk) = take_chunk(&mut track.pending, flush) {
        let at = track.consumed;
        track.consumed += chunk.len();
        if !is_speech(&chunk) {
            continue;
        }
        match session.run(&chunk, &transcribe_cpp::RunOptions::default()) {
            Ok(t) if !t.text.trim().is_empty() => out.line(at, track.label, t.text.trim())?,
            Ok(_) => {}
            Err(e) => log::warn!("meeting: transcribing a chunk failed: {e}"),
        }
    }
    Ok(())
}

/// The next chunk to transcribe: nothing while under 30 s (unless flushing at the end), otherwise
/// up to the quietest moment in the last few seconds of the first 30.
fn take_chunk(pending: &mut Vec<f32>, flush: bool) -> Option<Vec<f32>> {
    if pending.len() >= CHUNK {
        let cut = quietest_cut(&pending[..CHUNK]);
        return Some(pending.drain(..cut).collect());
    }
    if flush && !pending.is_empty() {
        return Some(std::mem::take(pending));
    }
    None
}

/// The middle of the quietest 100 ms frame in the window's last `SEARCH` samples.
fn quietest_cut(window: &[f32]) -> usize {
    let energy = |start: usize| window[start..start + FRAME].iter().map(|s| s * s).sum::<f32>();
    let quietest = (window.len() - SEARCH..=window.len() - FRAME)
        .step_by(FRAME)
        .min_by(|a, b| energy(*a).total_cmp(&energy(*b)))
        .unwrap_or(window.len() - FRAME);
    quietest + FRAME / 2
}

fn is_speech(chunk: &[f32]) -> bool {
    if chunk.len() < RATE / 2 {
        return false;
    }
    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
    rms >= SILENCE_RMS
}

/// The transcript file, written a line at a time so a crash loses at most the chunk in progress.
struct Transcript {
    file: std::fs::File,
    started: chrono::DateTime<chrono::Local>,
}

impl Transcript {
    fn create(path: &Path, started: chrono::DateTime<chrono::Local>) -> Result<Self> {
        crate::history::create_private_dir(path.parent().context("transcript has no folder")?)?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(path).with_context(|| format!("creating {}", path.display()))?;
        let mut t = Self { file, started };
        t.write(&format!(
            "Meeting transcript from stfu, started {}.\n\"You\" is my microphone and \"Others\" is the call. \
             Transcribed on this machine in chunks of about 30 seconds, so a line can end mid-sentence.\n\n",
            started.format("%A %-d %B %Y at %H:%M")
        ))?;
        Ok(t)
    }

    fn line(&mut self, at: usize, who: &str, text: &str) -> Result<()> {
        self.write(&format!("[{}] {who}: {text}\n", clock(at)))
    }

    fn finish(&mut self) -> Result<()> {
        let now = chrono::Local::now();
        let minutes = (now - self.started).num_minutes();
        self.write(&format!("\nEnded at {} ({minutes} min).\n", now.format("%H:%M")))
    }

    fn write(&mut self, text: &str) -> Result<()> {
        self.file.write_all(text.as_bytes()).context("writing the meeting transcript")
    }
}

/// Samples from the start of the meeting as `HH:MM:SS`.
fn clock(samples: usize) -> String {
    let s = samples / RATE;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize).map(|i| (i as f32 * 0.05).sin() * 0.3).collect()
    }

    #[test]
    fn nothing_is_cut_before_thirty_seconds() {
        let mut pending = tone(29.0);
        assert_eq!(take_chunk(&mut pending, false), None);
        assert_eq!(pending.len(), 29 * RATE);
    }

    #[test]
    fn a_chunk_ends_in_the_pause_before_the_thirty_second_mark() {
        let mut pending = tone(40.0);
        // Half a second of silence starting at 26 s.
        pending[26 * RATE..26 * RATE + RATE / 2].iter_mut().for_each(|s| *s = 0.0);
        let chunk = take_chunk(&mut pending, false).unwrap();
        let cut = chunk.len() as f32 / RATE as f32;
        assert!((26.0..26.5).contains(&cut), "cut at {cut} s");
        assert_eq!(chunk.len() + pending.len(), 40 * RATE);
    }

    #[test]
    fn continuous_speech_is_still_cut_within_thirty_seconds() {
        let mut pending = tone(65.0);
        let first = take_chunk(&mut pending, false).unwrap();
        assert!(first.len() > 24 * RATE && first.len() <= CHUNK);
        let second = take_chunk(&mut pending, false).unwrap();
        assert!(second.len() > 24 * RATE && second.len() <= CHUNK);
    }

    #[test]
    fn flushing_hands_over_the_rest() {
        let mut pending = tone(12.5);
        assert_eq!(take_chunk(&mut pending, true).map(|c| c.len()), Some(12 * RATE + RATE / 2));
        assert!(pending.is_empty());
        assert_eq!(take_chunk(&mut pending, true), None);
    }

    #[test]
    fn silence_and_slivers_are_not_speech() {
        assert!(!is_speech(&vec![0.0; 10 * RATE]));
        assert!(!is_speech(&tone(0.2)));
        assert!(is_speech(&tone(3.0)));
    }

    #[test]
    fn timestamps_count_from_the_start() {
        assert_eq!(clock(0), "00:00:00");
        assert_eq!(clock(95 * RATE), "00:01:35");
        assert_eq!(clock((3600 + 61) * RATE), "01:01:01");
    }

    /// Downloads the real model into the app's folder: `cargo test -- --ignored downloads`.
    #[test]
    #[ignore]
    fn downloads_and_verifies_the_model() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(download_model(|done, total| {
            if done == total {
                println!("downloaded {done} bytes");
            }
        }))
        .unwrap();
        assert!(model_ready());
        let mut sha = Sha256::new();
        sha.update(&std::fs::read(model_path().unwrap()).unwrap());
        assert_eq!(sha.finish(), MODEL.sha256);
    }

    /// Runs a 16 kHz mono WAV through the same chunking and model as a meeting, as the "Others"
    /// track, and prints the transcript: `STFU_MEETING_WAV=talk.wav cargo test -- --ignored --nocapture transcribes`.
    #[test]
    #[ignore]
    fn transcribes_a_recording() {
        let wav = std::env::var("STFU_MEETING_WAV").expect("set STFU_MEETING_WAV");
        let samples: Vec<f32> = hound::WavReader::open(&wav)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let seconds = samples.len() as f32 / RATE as f32;
        let model = transcribe_cpp::Model::load(model_path().unwrap()).unwrap();
        let mut session = new_session(&model).unwrap();
        let path = std::env::temp_dir().join(format!("stfu-meeting-test-{}.txt", std::process::id()));
        let mut out = Transcript::create(&path, chrono::Local::now()).unwrap();
        let mut track = Track { label: "Others", pending: samples, consumed: 0 };
        let t = std::time::Instant::now();
        transcribe_pending(&mut session, &mut track, true, &mut out).unwrap();
        out.finish().unwrap();
        let elapsed = t.elapsed().as_secs_f32();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        println!("{text}\n{seconds:.1} s of audio in {elapsed:.1} s ({:.1}x real time)", seconds / elapsed);
        assert!(text.lines().filter(|l| l.starts_with('[')).count() >= (seconds / 30.0) as usize);
    }

    /// A live meeting on this machine, with real capture: `STFU_MEETING_APPS=com.apple.Safari
    /// STFU_MEETING_SECONDS=60 cargo test -- --ignored --nocapture live`. Needs Microphone and
    /// Screen Recording for whatever runs the test.
    #[test]
    #[ignore]
    fn live_meeting() {
        let apps = std::env::var("STFU_MEETING_APPS").expect("set STFU_MEETING_APPS");
        let seconds: u64 = std::env::var("STFU_MEETING_SECONDS").map_or(45, |s| s.parse().unwrap());
        let path = start(apps.split(',').map(String::from).collect(), |e| panic!("{e:#}")).unwrap();
        assert!(running());
        std::thread::sleep(Duration::from_secs(seconds));
        assert_eq!(stop().unwrap(), path);
        assert!(!running());
        println!("{}", std::fs::read_to_string(&path).unwrap());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn sha256_matches_a_known_digest() {
        let mut sha = Sha256::new();
        sha.update(b"abc");
        assert_eq!(sha.finish(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
