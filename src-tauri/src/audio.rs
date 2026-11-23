//! Microphone capture on a dedicated thread. `cpal::Stream` is not guaranteed to be `Send`,
//! so the stream lives on its own thread and is driven by commands over a channel.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
pub const TARGET_RATE: u32 = 16_000;

/// Levels per second of audio, one bar each on the pill (main.ts `LEVEL_MS` must match).
const LEVELS_PER_SECOND: u32 = 30;
/// Levels per message to the pill. Each message wakes the pill's page and redraws it; pairs halve
/// that work, and the pill's scroll runs far enough behind the newest level to hide the wait.
const LEVELS_PER_MESSAGE: usize = 2;

enum Cmd {
    Start(Sender<Vec<f32>>),
    Stop(Sender<Result<Vec<u8>>>),
}

#[derive(Clone)]
pub struct Recorder {
    tx: Sender<Cmd>,
}

impl Recorder {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || audio_thread(rx))
            .expect("spawn audio thread");
        Self { tx }
    }

    /// Starts capturing. Peak levels (0..1), one per 1/30 s of audio, arrive on the returned
    /// receiver in pairs, oldest first.
    pub fn start(&self) -> Result<Receiver<Vec<f32>>> {
        let (ltx, lrx) = mpsc::channel();
        self.tx.send(Cmd::Start(ltx)).map_err(|_| anyhow!("audio thread gone"))?;
        Ok(lrx)
    }

    /// Stops capturing and returns a 16 kHz mono 16-bit WAV.
    pub fn stop(&self) -> Result<Vec<u8>> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.send(Cmd::Stop(rtx)).map_err(|_| anyhow!("audio thread gone"))?;
        rrx.recv().map_err(|_| anyhow!("audio thread gone"))?
    }
}

struct Active {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
    channels: u16,
}

fn audio_thread(rx: Receiver<Cmd>) {
    let mut active: Option<Active> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start(level_tx) => {
                active = None; // drop any previous stream
                match start_stream(level_tx) {
                    Ok(a) => active = Some(a),
                    Err(e) => log::error!("could not start microphone: {e:#}"),
                }
            }
            Cmd::Stop(reply) => {
                let result = match active.take() {
                    Some(a) => {
                        let samples = std::mem::take(&mut *a.samples.lock().unwrap());
                        encode_wav(&samples, a.rate, a.channels)
                    }
                    None => Err(anyhow!("microphone was not recording")),
                };
                let _ = reply.send(result);
            }
        }
    }
}

fn start_stream(level_tx: Sender<Vec<f32>>) -> Result<Active> {
    let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(48_000 * 30)));
    let sink = samples.clone();
    let (stream, rate, channels) = open_input_with(move |rate, channels| {
        let mut levels = Levels::new(rate, channels);
        move |data: &[f32]| {
            sink.lock().unwrap().extend_from_slice(data);
            for batch in levels.feed(data) {
                let _ = level_tx.send(batch);
            }
        }
    })?;
    Ok(Active { _stream: stream, samples, rate, channels })
}

/// Opens the default microphone and calls `on_data` with its samples as f32, interleaved at the
/// device's own rate and channel count, which are returned alongside the running stream.
pub fn open_input(
    on_data: impl FnMut(&[f32]) + Send + 'static,
) -> Result<(cpal::Stream, u32, u16)> {
    open_input_with(move |_, _| on_data)
}

/// Like `open_input`, but builds the callback once the device's rate and channel count are known.
fn open_input_with<F: FnMut(&[f32]) + Send + 'static>(
    make: impl FnOnce(u32, u16) -> F,
) -> Result<(cpal::Stream, u32, u16)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default input device (is a microphone connected and allowed?)")?;
    let config = device
        .default_input_config()
        .context("querying default input config")?;
    log::info!(
        "recording from '{}' at {} Hz, {} ch, {:?}",
        device.description().map(|d| d.name().to_string()).unwrap_or_default(),
        config.sample_rate(),
        config.channels(),
        config.sample_format()
    );
    let rate = config.sample_rate();
    let channels = config.channels();
    let mut on_data = make(rate, channels);

    let err_fn = |e| log::error!("audio stream error: {e}");
    let stream_config: cpal::StreamConfig = config.clone().into();
    let mut scratch = Vec::<f32>::new();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            stream_config.clone(),
            move |data: &[f32], _| on_data(data),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            stream_config.clone(),
            move |data: &[i16], _| {
                scratch.clear();
                scratch.extend(data.iter().map(|s| *s as f32 / i16::MAX as f32));
                on_data(&scratch)
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            stream_config.clone(),
            move |data: &[u16], _| {
                scratch.clear();
                scratch.extend(data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0));
                on_data(&scratch)
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported sample format {other:?}")),
    };
    stream.play().context("starting input stream")?;
    Ok((stream, rate, channels))
}

/// Turns the microphone's samples into the pill's levels: the peak of each 1/30 s of audio, by
/// sample count rather than wall clock, so the pill gets an even 30 a second however the device
/// sizes its buffers.
struct Levels {
    /// Interleaved samples per level.
    window: usize,
    left: usize,
    peak: f32,
    pending: Vec<f32>,
}

impl Levels {
    fn new(rate: u32, channels: u16) -> Self {
        let window = ((rate / LEVELS_PER_SECOND) as usize * channels.max(1) as usize).max(1);
        Self { window, left: window, peak: 0.0, pending: Vec::with_capacity(LEVELS_PER_MESSAGE) }
    }

    /// Takes the next samples and returns the batches of levels that are now complete.
    fn feed(&mut self, data: &[f32]) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        for &s in data {
            self.peak = self.peak.max(s.abs());
            self.left -= 1;
            if self.left == 0 {
                self.pending.push(self.peak);
                self.peak = 0.0;
                self.left = self.window;
                if self.pending.len() == LEVELS_PER_MESSAGE {
                    out.push(std::mem::replace(&mut self.pending, Vec::with_capacity(LEVELS_PER_MESSAGE)));
                }
            }
        }
        out
    }
}

/// Whether a usable input device exists. On Windows a blocked microphone privacy setting shows up
/// here, since the device cannot be opened for capture.
pub fn input_device_available() -> bool {
    let host = cpal::default_host();
    match host.default_input_device() {
        Some(d) => d.default_input_config().is_ok(),
        None => false,
    }
}

/// A WAV of near-silence, used to validate speech-to-text credentials without a microphone.
pub fn silent_wav(seconds: f32) -> Result<Vec<u8>> {
    let n = (TARGET_RATE as f32 * seconds) as usize;
    let samples: Vec<f32> = (0..n).map(|i| if i % 97 == 0 { 0.002 } else { 0.0 }).collect();
    encode_wav(&samples, TARGET_RATE, 1)
}

/// Downmixes interleaved samples to mono and resamples them to 16 kHz (linear).
pub fn to_mono_16k(samples: &[f32], rate: u32, channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect();
    if rate == TARGET_RATE {
        return mono;
    }
    let ratio = rate as f64 / TARGET_RATE as f64;
    let out_len = (mono.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = mono.get(idx).copied().unwrap_or(0.0);
            let b = mono.get(idx + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

/// Downmixes to mono, resamples to 16 kHz (linear), writes 16-bit PCM WAV.
fn encode_wav(samples: &[f32], rate: u32, channels: u16) -> Result<Vec<u8>> {
    let resampled = to_mono_16k(samples, rate, channels);
    // Too short, or nothing but silence (also what macOS delivers when the mic is not allowed):
    // return an empty WAV so the caller can skip quietly instead of showing an error.
    let peak = resampled.iter().fold(0f32, |m, s| m.max(s.abs()));
    if resampled.len() < TARGET_RATE as usize / 4 || peak < 0.0005 {
        log::info!("skipping take: {} samples, peak {peak:.4}", resampled.len());
        return Ok(Vec::new());
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut cursor, spec)?;
        for s in resampled {
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        w.finalize()?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_come_thirty_a_second_in_pairs_however_buffers_are_sized() {
        // One second of 48 kHz stereo in odd-sized device buffers.
        let mut levels = Levels::new(48_000, 2);
        let audio: Vec<f32> = (0..96_000).map(|i| if i / 3200 == 7 { 0.5 } else { 0.01 }).collect();
        let batches: Vec<Vec<f32>> = audio.chunks(941).flat_map(|c| levels.feed(c)).collect();
        assert_eq!(batches.len(), 15);
        assert!(batches.iter().all(|b| b.len() == 2));
        let flat: Vec<f32> = batches.concat();
        // Each level is the peak of its own 1/30 s: only the eighth one saw the loud stretch.
        assert_eq!(flat[7], 0.5);
        assert!(flat.iter().enumerate().all(|(i, l)| i == 7 || *l == 0.01));
    }
}
