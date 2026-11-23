//! Whisper's answers to silence. Given a take with no speech in it (the hotkey held, nothing
//! said), Whisper does not return nothing: it returns one of a few stock phrases from its
//! subtitle training data, by far most often "Thank you.". Such a transcript is dropped when the
//! audio shows nobody spoke: it is too quiet, or it has (almost) none of the steady pitch of a
//! voice.
//!
//! MEASURED on 2,228 real takes from the history: all 17 "Thank you." and both "you" transcripts
//! of silent takes were too quiet (loudest 30 ms stretch at most 0.0088 RMS) and are dropped; none
//! of the 2,197 real dictations would have been, because none is one of these phrases. Loudness
//! alone cannot be used: real quiet words ("Wait.", "Thread.") are quieter than some silent
//! takes. Whisper's own `no_speech_prob` is no help either: Groq reports 0 for these takes.

/// Transcripts Whisper writes for silence, compared after `normalise`.
const PHRASES: &[&str] = &[
    "thank you",
    "thank you very much",
    "thank you so much",
    "thank you for watching",
    "thanks for watching",
    "thank you for watching and see you next time",
    "thanks",
    "you",
    "bye",
    "bye bye",
    "please subscribe",
    "bedankt voor het kijken",
    "bedankt",
    "dank je wel",
];

/// Whisper's subtitle credit, in any language ("Subtitles by the Amara.org community").
const CREDIT: &str = "amara org";

/// The loudest 30 ms of a take with speech in it is louder than this (RMS, full scale 1.0).
const QUIET: f32 = 0.012;
/// A take with speech in it has at least this many strongly voiced 30 ms frames.
const MIN_VOICED: usize = 3;

const RATE: usize = 16_000;
/// 30 ms frames.
const FRAME: usize = 480;
/// Pitch periods searched, in samples at 16 kHz: 400 Hz down to 60 Hz.
const LAGS: std::ops::RangeInclusive<usize> = 40..=267;
/// Frames quieter than this are not searched for pitch.
const FLOOR: f32 = 0.0015;
/// How closely a frame must repeat itself one pitch period later to count as voiced (0..1).
const VOICED: f32 = 0.75;
/// Only the start of a long take is analysed; speech anywhere in it shows up well before this.
const MAX_SECONDS: usize = 30;

/// True when `transcript` is Whisper's answer to silence and the take's audio (a 16 kHz mono WAV)
/// has no voice in it.
pub fn heard_nothing(transcript: &str, wav: &[u8]) -> bool {
    if !is_stock_phrase(transcript) {
        return false;
    }
    let Some(samples) = read_wav(wav) else {
        return false;
    };
    let voice = Voice::of(&samples);
    log::info!(
        "stock phrase {transcript:?}: loudest {:.4} RMS, {} voiced frames",
        voice.loudest,
        voice.voiced
    );
    voice.loudest < QUIET || voice.voiced < MIN_VOICED
}

fn is_stock_phrase(transcript: &str) -> bool {
    let text = normalise(transcript);
    PHRASES.contains(&text.as_str()) || text.contains(CREDIT)
}

/// Lowercase words separated by single spaces; punctuation and case dropped.
fn normalise(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 16-bit WAV samples as f32. `None` if it is not one (the take is then left alone).
fn read_wav(wav: &[u8]) -> Option<Vec<f32>> {
    let reader = hound::WavReader::new(std::io::Cursor::new(wav)).ok()?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 || spec.channels != 1 {
        return None;
    }
    let samples: Result<Vec<f32>, _> = reader
        .into_samples::<i16>()
        .take(RATE * MAX_SECONDS)
        .map(|s| s.map(|s| s as f32 / i16::MAX as f32))
        .collect();
    samples.ok()
}

/// What a take's audio says about a voice being in it.
#[derive(Debug, Default, PartialEq)]
struct Voice {
    /// RMS of the loudest 30 ms frame.
    loudest: f32,
    /// Frames that repeat themselves at a pitch period, as voiced speech does.
    voiced: usize,
}

impl Voice {
    fn of(samples: &[f32]) -> Self {
        let mut voice = Voice::default();
        let mut frame = [0f32; FRAME];
        for chunk in samples.chunks_exact(FRAME) {
            let mean = chunk.iter().sum::<f32>() / FRAME as f32;
            for (f, s) in frame.iter_mut().zip(chunk) {
                *f = s - mean;
            }
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt();
            voice.loudest = voice.loudest.max(rms);
            if rms >= FLOOR && periodicity(&frame) >= VOICED {
                voice.voiced += 1;
            }
        }
        voice
    }
}

/// The best normalised autocorrelation over the pitch lags: near 1 for a voiced vowel, low for
/// noise, clicks and breath.
fn periodicity(frame: &[f32]) -> f32 {
    let mut best = 0f32;
    for lag in LAGS {
        let (a, b) = (&frame[..frame.len() - lag], &frame[lag..]);
        let (mut num, mut ea, mut eb) = (0f32, 0f32, 0f32);
        for (x, y) in a.iter().zip(b) {
            num += x * y;
            ea += x * x;
            eb += y * y;
        }
        let denom = (ea * eb).sqrt();
        if denom > 0.0 {
            best = best.max(num / denom);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV the way audio.rs writes takes: 16 kHz mono 16-bit.
    fn wav(samples: &[f32]) -> Vec<u8> {
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut out = std::io::Cursor::new(Vec::new());
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for s in samples {
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).unwrap();
        }
        w.finalize().unwrap();
        out.into_inner()
    }

    /// Deterministic noise in -1..1.
    fn noise(n: usize, amplitude: f32) -> Vec<f32> {
        let mut x = 0x2545_f491u32;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
            })
            .collect()
    }

    /// A vowel-like sound: a 140 Hz voice with a few harmonics, under a little noise.
    fn vowel(seconds: f32, amplitude: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let hiss = noise(n, amplitude * 0.1);
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let w = 2.0 * std::f32::consts::PI * 140.0 * t;
                amplitude * (0.6 * w.sin() + 0.3 * (2.0 * w).sin() + 0.1 * (3.0 * w).sin()) + hiss[i]
            })
            .collect()
    }

    #[test]
    fn stock_phrases_are_recognised_whatever_the_punctuation() {
        for t in ["Thank you.", " thank you!", "Thank you very much.", "you", "Bye-bye.", "Thanks for watching!",
                  "Subtitles by the Amara.org community", "Bedankt voor het kijken."] {
            assert!(is_stock_phrase(t), "{t}");
        }
        for t in ["Thank you for the review.", "I thank you.", "Yes.", "Wait.", "Can you?", ""] {
            assert!(!is_stock_phrase(t), "{t}");
        }
    }

    #[test]
    fn thank_you_over_silence_or_mic_hiss_is_dropped() {
        assert!(heard_nothing("Thank you.", &wav(&vec![0.0; RATE])));
        assert!(heard_nothing("Thank you.", &wav(&noise(RATE, 0.004))));
        assert!(heard_nothing("you", &wav(&noise(RATE / 2, 0.006))));
    }

    #[test]
    fn loud_noise_without_a_voice_is_dropped_too() {
        // A key click or a bump: loud, but nothing voiced.
        let mut s = noise(RATE, 0.003);
        s[8000..8400].copy_from_slice(&noise(400, 0.4));
        assert!(heard_nothing("Thank you.", &wav(&s)));
    }

    #[test]
    fn a_spoken_thank_you_is_kept() {
        let mut s = noise(RATE / 4, 0.003);
        s.extend(vowel(0.5, 0.05));
        s.extend(noise(RATE / 4, 0.003));
        assert!(!heard_nothing("Thank you.", &wav(&s)));
    }

    #[test]
    fn anything_else_is_never_touched_however_quiet() {
        assert!(!heard_nothing("Wait.", &wav(&vec![0.0; RATE])));
        assert!(!heard_nothing("Thank you for the review.", &wav(&noise(RATE, 0.002))));
    }

    #[test]
    fn an_unreadable_wav_is_left_alone() {
        assert!(!heard_nothing("Thank you.", b"not a wav"));
    }

    #[test]
    fn a_voice_is_periodic_and_noise_is_not() {
        let v = Voice::of(&vowel(0.5, 0.05));
        assert!(v.voiced >= 10, "{v:?}");
        assert!(v.loudest > QUIET, "{v:?}");
        let n = Voice::of(&noise(RATE / 2, 0.05));
        assert!(n.voiced < MIN_VOICED, "{n:?}");
    }
}
