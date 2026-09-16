//! Microphone capture on a dedicated thread. `cpal::Stream` is not guaranteed to be `Send`,
//! so the stream lives on its own thread and is driven by commands over a channel.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TARGET_RATE: u32 = 16_000;

enum Cmd {
    Start(Sender<f32>),
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

    /// Starts capturing. Peak levels (0..1) are sent on the returned receiver ~30x per second.
    pub fn start(&self) -> Result<Receiver<f32>> {
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

fn start_stream(level_tx: Sender<f32>) -> Result<Active> {
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
    let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(rate as usize * 30)));
    let sink = samples.clone();
    let mut last_emit = Instant::now();
    let mut peak = 0f32;

    let err_fn = |e| log::error!("audio stream error: {e}");
    let stream_config: cpal::StreamConfig = config.clone().into();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            stream_config.clone(),
            move |data: &[f32], _| {
                push(&sink, data.iter().copied(), &mut peak, &mut last_emit, &level_tx)
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            stream_config.clone(),
            move |data: &[i16], _| {
                push(
                    &sink,
                    data.iter().map(|s| *s as f32 / i16::MAX as f32),
                    &mut peak,
                    &mut last_emit,
                    &level_tx,
                )
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            stream_config.clone(),
            move |data: &[u16], _| {
                push(
                    &sink,
                    data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0),
                    &mut peak,
                    &mut last_emit,
                    &level_tx,
                )
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported sample format {other:?}")),
    };
    stream.play().context("starting input stream")?;
    Ok(Active { _stream: stream, samples, rate, channels })
}

fn push(
    sink: &Arc<Mutex<Vec<f32>>>,
    data: impl Iterator<Item = f32>,
    peak: &mut f32,
    last_emit: &mut Instant,
    level_tx: &Sender<f32>,
) {
    let mut buf = sink.lock().unwrap();
    for s in data {
        *peak = peak.max(s.abs());
        buf.push(s);
    }
    if last_emit.elapsed() >= Duration::from_millis(33) {
        let _ = level_tx.send(*peak);
        *peak = 0.0;
        *last_emit = Instant::now();
    }
}

/// A WAV of near-silence, used to validate speech-to-text credentials without a microphone.
pub fn silent_wav(seconds: f32) -> Result<Vec<u8>> {
    let n = (TARGET_RATE as f32 * seconds) as usize;
    let samples: Vec<f32> = (0..n).map(|i| if i % 97 == 0 { 0.002 } else { 0.0 }).collect();
    encode_wav(&samples, TARGET_RATE, 1)
}

/// Downmixes to mono, resamples to 16 kHz (linear), trims edge silence, writes 16-bit PCM WAV.
fn encode_wav(samples: &[f32], rate: u32, channels: u16) -> Result<Vec<u8>> {
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect();
    let resampled = if rate == TARGET_RATE {
        mono
    } else {
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
    };
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
