//! Lossless compression for kept recordings: a small FLAC encoder and a decoder for what it
//! writes. No dependency, and the files are standard FLAC, so any player opens them.
//!
//! The encoder uses FLAC's fixed predictors (orders 0-4) with partitioned Rice coding, the same
//! tools `flac -0`..`-2` use. On speech that roughly halves a 16-bit WAV (measured on real takes:
//! zip saves ~22%, this ~45%), with every sample coming back bit for bit.
//!
//! Only what the app records is supported: 16-bit PCM WAV, 1 to 8 channels. The decoder reads
//! exactly the subset the encoder writes and refuses anything else rather than guess.

use anyhow::{anyhow, bail, ensure, Result};

const BLOCK: usize = 4096;
const MAX_PARTITION_ORDER: u32 = 6;
const BPS: u32 = 16;

/// Interleaved 16-bit samples and their format.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

/// Reads a 16-bit PCM WAV, walking its chunks rather than assuming a fixed 44-byte header.
pub fn parse_wav(wav: &[u8]) -> Result<Pcm> {
    ensure!(wav.len() >= 12 && &wav[0..4] == b"RIFF" && &wav[8..12] == b"WAVE", "not a WAV file");
    let mut at = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let len = u32::from_le_bytes(wav[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &wav[at + 8..(at + 8 + len).min(wav.len())];
        if id == b"fmt " {
            ensure!(body.len() >= 16, "short fmt chunk");
            let u16_at = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
            let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            format = Some((u16_at(0), u16_at(2), rate, u16_at(14)));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format.ok_or_else(|| anyhow!("data before fmt"))?;
            ensure!(tag == 1 && bits == 16, "only 16-bit PCM is compressed (format {tag}, {bits} bits)");
            ensure!((1..=8).contains(&channels), "unsupported channel count {channels}");
            ensure!(rate > 0 && rate < 1 << 20, "unsupported sample rate {rate}");
            let samples = body.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            return Ok(Pcm { rate, channels, samples });
        }
        at += 8 + len + (len & 1);
    }
    bail!("no audio data in WAV")
}

/// A canonical 44-byte-header 16-bit PCM WAV, the same layout the recorder writes.
pub fn to_wav(pcm: &Pcm) -> Vec<u8> {
    let data_len = (pcm.samples.len() * 2) as u32;
    let block_align = pcm.channels as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&pcm.channels.to_le_bytes());
    out.extend_from_slice(&pcm.rate.to_le_bytes());
    out.extend_from_slice(&(pcm.rate * block_align).to_le_bytes());
    out.extend_from_slice(&(block_align as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &pcm.samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// WAV in, FLAC out.
pub fn encode_wav(wav: &[u8]) -> Result<Vec<u8>> {
    encode(&parse_wav(wav)?)
}

/// FLAC in, WAV out.
pub fn decode_to_wav(flac: &[u8]) -> Result<Vec<u8>> {
    Ok(to_wav(&decode(flac)?))
}

pub fn encode(pcm: &Pcm) -> Result<Vec<u8>> {
    let ch = pcm.channels as usize;
    ensure!((1..=8).contains(&ch), "unsupported channel count {ch}");
    ensure!(pcm.samples.len() % ch == 0, "sample count is not a multiple of the channel count");
    let frames = pcm.samples.len() / ch;
    ensure!(frames >= 16, "too short to compress");
    let block = BLOCK.min(frames);

    let mut w = BitWriter::default();
    w.out.extend_from_slice(b"fLaC");
    // STREAMINFO, the only metadata block, flagged as the last one.
    w.put(1, 1);
    w.put(7, 0);
    w.put(24, 34);
    w.put(16, block as u64);
    w.put(16, block as u64);
    w.put(24, 0); // min frame size: unknown
    w.put(24, 0); // max frame size: unknown
    w.put(20, pcm.rate as u64);
    w.put(3, ch as u64 - 1);
    w.put(5, BPS as u64 - 1);
    w.put(36, frames as u64);
    for _ in 0..4 {
        w.put(32, 0); // MD5 of the audio: optional, left as "not computed"
    }

    let mut channel = vec![0i32; block];
    for (number, start) in (0..frames).step_by(block).enumerate() {
        let n = block.min(frames - start);
        let frame_start = w.out.len();
        frame_header(&mut w, n, pcm.rate, ch, number as u64);
        for c in 0..ch {
            for (i, s) in channel[..n].iter_mut().enumerate() {
                *s = pcm.samples[(start + i) * ch + c] as i32;
            }
            subframe(&mut w, &channel[..n]);
        }
        w.align();
        let crc = crc16(&w.out[frame_start..]);
        w.put(16, crc as u64);
    }
    Ok(w.out)
}

fn frame_header(w: &mut BitWriter, n: usize, rate: u32, channels: usize, number: u64) {
    let start = w.out.len();
    w.put(14, 0x3FFE); // sync code
    w.put(1, 0);
    w.put(1, 0); // fixed block size; the header carries the frame number
    let (size_code, extra) = match n {
        4096 => (12, None),
        1..=256 => (6, Some((8, n as u64 - 1))),
        _ => (7, Some((16, n as u64 - 1))),
    };
    w.put(4, size_code);
    w.put(4, rate_code(rate));
    w.put(4, channels as u64 - 1); // independent channels
    w.put(3, 0b100); // 16 bits per sample
    w.put(1, 0);
    for b in utf8_number(number) {
        w.put(8, b as u64);
    }
    if let Some((bits, v)) = extra {
        w.put(bits, v);
    }
    let crc = crc8(&w.out[start..]);
    w.put(8, crc as u64);
}

/// Sample rates the frame header can name; anything else defers to STREAMINFO.
fn rate_code(rate: u32) -> u64 {
    match rate {
        8000 => 4,
        16000 => 5,
        22050 => 6,
        24000 => 7,
        32000 => 8,
        44100 => 9,
        48000 => 10,
        96000 => 11,
        _ => 0,
    }
}

/// FLAC numbers frames with the UTF-8 byte pattern, extended to 36 bits.
fn utf8_number(v: u64) -> Vec<u8> {
    if v < 0x80 {
        return vec![v as u8];
    }
    let len = match v {
        0..=0x7FF => 2,
        0x800..=0xFFFF => 3,
        0x1_0000..=0x1F_FFFF => 4,
        0x20_0000..=0x3FF_FFFF => 5,
        0x400_0000..=0x7FFF_FFFF => 6,
        _ => 7,
    };
    let mut out = vec![0u8; len];
    let mut v = v;
    for b in out[1..].iter_mut().rev() {
        *b = 0x80 | (v & 0x3F) as u8;
        v >>= 6;
    }
    let lead = (0xFF00u16 >> len) as u8; // len ones followed by a zero
    out[0] = if len == 7 { 0xFE } else { lead | v as u8 };
    out
}

/// One channel of one frame. Silence becomes a CONSTANT subframe; otherwise the best fixed
/// predictor with Rice-coded residuals, or the raw samples if coding would not help.
fn subframe(w: &mut BitWriter, x: &[i32]) {
    let n = x.len();
    if x.iter().all(|&s| s == x[0]) {
        w.put(8, 0); // padding bit, CONSTANT, no wasted bits
        w.put_signed(BPS, x[0] as i64);
        return;
    }
    let order = best_fixed_order(x);
    let residual = fixed_residual(x, order);
    let (partition_order, params, residual_bits) = best_partitioning(&residual, n, order);
    let coded = 8 + order as u64 * BPS as u64 + 6 + residual_bits;
    if coded >= 8 + n as u64 * BPS as u64 {
        w.put(8, 0b0000_0010); // VERBATIM
        for &s in x {
            w.put_signed(BPS, s as i64);
        }
        return;
    }
    w.put(8, 0b0001_0000 | (order as u64) << 1); // FIXED: type 001xxx, xxx = order
    for &s in &x[..order] {
        w.put_signed(BPS, s as i64);
    }
    w.put(2, 0); // Rice coding, 4-bit parameters
    w.put(4, partition_order as u64);
    let per = n >> partition_order;
    let mut at = 0;
    for (p, &k) in params.iter().enumerate() {
        let len = if p == 0 { per - order } else { per };
        w.put(4, k as u64);
        for &r in &residual[at..at + len] {
            let u = fold(r);
            w.unary(u >> k);
            w.put(k, (u & ((1 << k) - 1)) as u64);
        }
        at += len;
    }
}

/// Signed to unsigned, interleaving: 0, -1, 1, -2, 2 ... -> 0, 1, 2, 3, 4 ...
fn fold(r: i32) -> u32 {
    ((r << 1) ^ (r >> 31)) as u32
}

fn unfold(u: u32) -> i32 {
    ((u >> 1) as i32) ^ -((u & 1) as i32)
}

/// Picks the predictor order with the smallest total residual, the reference encoder's rule.
fn best_fixed_order(x: &[i32]) -> usize {
    let mut sums = [0u64; 5];
    for i in 4..x.len() {
        let e0 = x[i] as i64;
        let e1 = e0 - x[i - 1] as i64;
        let e2 = e1 - (x[i - 1] as i64 - x[i - 2] as i64);
        let e3 = e2 - (x[i - 1] as i64 - 2 * x[i - 2] as i64 + x[i - 3] as i64);
        let e4 = e3 - (x[i - 1] as i64 - 3 * x[i - 2] as i64 + 3 * x[i - 3] as i64 - x[i - 4] as i64);
        for (s, e) in sums.iter_mut().zip([e0, e1, e2, e3, e4]) {
            *s += e.unsigned_abs();
        }
    }
    let max = if x.len() > 4 { 4 } else { x.len() - 1 };
    (0..=max).min_by_key(|&o| sums[o]).unwrap_or(0)
}

/// The prediction error for each sample after the `order` warm-up samples.
fn fixed_residual(x: &[i32], order: usize) -> Vec<i32> {
    (order..x.len())
        .map(|i| {
            let p = match order {
                0 => 0,
                1 => x[i - 1],
                2 => 2 * x[i - 1] - x[i - 2],
                3 => 3 * x[i - 1] - 3 * x[i - 2] + x[i - 3],
                _ => 4 * x[i - 1] - 6 * x[i - 2] + 4 * x[i - 3] - x[i - 4],
            };
            x[i] - p
        })
        .collect()
}

/// Cheapest Rice parameter for a partition from the sum of its folded residuals, and its cost
/// in bits. Uses the same estimate as the reference encoder: n * (k + 1) + sum >> k.
fn rice_param(sum: u64, len: usize) -> (u32, u64) {
    (0..=14u32)
        .map(|k| (k, len as u64 * (k as u64 + 1) + (sum >> k)))
        .min_by_key(|&(_, bits)| bits)
        .unwrap()
}

/// Tries every allowed partition order and keeps the cheapest: (order, Rice parameters, bits).
fn best_partitioning(residual: &[i32], n: usize, order: usize) -> (u32, Vec<u32>, u64) {
    let mut best: Option<(u32, Vec<u32>, u64)> = None;
    for p in 0..=MAX_PARTITION_ORDER {
        if n % (1 << p) != 0 || (n >> p) <= order {
            break;
        }
        let per = n >> p;
        let (mut params, mut bits, mut at) = (Vec::with_capacity(1 << p), 0u64, 0usize);
        for i in 0..(1usize << p) {
            let len = if i == 0 { per - order } else { per };
            let sum: u64 = residual[at..at + len].iter().map(|&r| fold(r) as u64).sum();
            let (k, cost) = rice_param(sum, len);
            params.push(k);
            bits += 4 + cost;
            at += len;
        }
        if best.as_ref().is_none_or(|b| bits < b.2) {
            best = Some((p, params, bits));
        }
    }
    best.expect("partition order 0 always fits")
}

pub fn decode(data: &[u8]) -> Result<Pcm> {
    ensure!(data.len() >= 42 && &data[0..4] == b"fLaC", "not a FLAC file");
    let mut r = BitReader::new(&data[4..]);
    let mut info: Option<(u32, u16, u64)> = None;
    loop {
        let last = r.get(1)? == 1;
        let kind = r.get(7)?;
        let len = r.get(24)? as usize;
        if kind == 0 {
            r.get(16)?;
            r.get(16)?;
            r.get(24)?;
            r.get(24)?;
            let rate = r.get(20)? as u32;
            let channels = r.get(3)? as u16 + 1;
            let bps = r.get(5)? + 1;
            let total = r.get(36)?;
            ensure!(bps == BPS as u64, "only 16-bit FLAC is supported");
            r.skip_bytes(16)?;
            info = Some((rate, channels, total));
        } else {
            r.skip_bytes(len)?;
        }
        if last {
            break;
        }
    }
    let (rate, channels, total) = info.ok_or_else(|| anyhow!("FLAC without STREAMINFO"))?;
    let ch = channels as usize;
    let mut samples = Vec::with_capacity(total as usize * ch);
    let mut planes: Vec<Vec<i32>> = vec![Vec::new(); ch];

    while (samples.len() as u64) < total * ch as u64 {
        let frame_start = r.byte_pos();
        ensure!(r.get(14)? == 0x3FFE, "lost frame sync");
        r.get(2)?;
        let size_code = r.get(4)?;
        let rate_code = r.get(4)?;
        ensure!(r.get(4)? as usize == ch - 1, "unsupported channel layout");
        ensure!(matches!(r.get(3)?, 0 | 0b100), "unsupported sample size");
        r.get(1)?;
        let lead = r.get(8)?;
        for _ in 0..(lead as u8).leading_ones().saturating_sub(1) {
            r.get(8)?;
        }
        let n = match size_code {
            1 => 192,
            2..=5 => 576 << (size_code - 2),
            6 => r.get(8)? as usize + 1,
            7 => r.get(16)? as usize + 1,
            8..=15 => 256 << (size_code - 8),
            _ => bail!("reserved block size"),
        };
        match rate_code {
            12 => {
                r.get(8)?;
            }
            13 | 14 => {
                r.get(16)?;
            }
            15 => bail!("invalid sample rate code"),
            _ => {}
        }
        let header_end = r.byte_pos();
        ensure!(r.get(8)? as u8 == crc8(&r.bytes()[frame_start..header_end]), "frame header is damaged");
        for plane in planes.iter_mut() {
            plane.clear();
            read_subframe(&mut r, n, plane)?;
        }
        r.align();
        let body_end = r.byte_pos();
        ensure!(r.get(16)? as u16 == crc16(&r.bytes()[frame_start..body_end]), "frame is damaged");
        for i in 0..n {
            for plane in &planes {
                samples.push(i16::try_from(plane[i]).map_err(|_| anyhow!("sample out of range"))?);
            }
        }
    }
    ensure!(samples.len() as u64 == total * ch as u64, "sample count does not match STREAMINFO");
    Ok(Pcm { rate, channels, samples })
}

fn read_subframe(r: &mut BitReader, n: usize, out: &mut Vec<i32>) -> Result<()> {
    ensure!(r.get(1)? == 0, "bad subframe padding");
    let kind = r.get(6)?;
    ensure!(r.get(1)? == 0, "wasted bits are not supported");
    match kind {
        0 => {
            let v = r.get_signed(BPS)? as i32;
            out.resize(n, v);
        }
        1 => {
            for _ in 0..n {
                out.push(r.get_signed(BPS)? as i32);
            }
        }
        8..=12 => {
            let order = (kind - 8) as usize;
            ensure!(order <= n, "predictor order longer than the block");
            for _ in 0..order {
                out.push(r.get_signed(BPS)? as i32);
            }
            let method = r.get(2)?;
            ensure!(method <= 1, "unsupported residual coding");
            let param_bits = if method == 0 { 4 } else { 5 };
            let escape = (1u64 << param_bits) - 1;
            let p = r.get(4)? as u32;
            let per = n >> p;
            ensure!(n % (1 << p) == 0 && per >= order, "bad partition order");
            for i in 0..(1usize << p) {
                let len = if i == 0 { per - order } else { per };
                let k = r.get(param_bits)?;
                if k == escape {
                    let bits = r.get(5)? as u32;
                    for _ in 0..len {
                        let res = if bits == 0 { 0 } else { r.get_signed(bits)? as i32 };
                        push_predicted(out, order, res);
                    }
                } else {
                    for _ in 0..len {
                        let q = r.unary()?;
                        let u = (q << k) | r.get(k as u32)? as u32;
                        push_predicted(out, order, unfold(u));
                    }
                }
            }
        }
        _ => bail!("unsupported subframe type {kind}"),
    }
    Ok(())
}

fn push_predicted(x: &mut Vec<i32>, order: usize, residual: i32) {
    let i = x.len();
    let p = match order {
        0 => 0,
        1 => x[i - 1],
        2 => 2 * x[i - 1] - x[i - 2],
        3 => 3 * x[i - 1] - 3 * x[i - 2] + x[i - 3],
        _ => 4 * x[i - 1] - 6 * x[i - 2] + 4 * x[i - 3] - x[i - 4],
    };
    x.push(p + residual);
}

fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
        }
    }
    crc
}

/// MSB-first bit packer.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    /// Writes the low `bits` bits of `v`, up to 36 at a time.
    fn put(&mut self, bits: u32, v: u64) {
        if bits > 32 {
            self.put(bits - 32, v >> 32);
            self.put(32, v);
            return;
        }
        if bits == 0 {
            return;
        }
        self.acc = (self.acc << bits) | (v & ((1u64 << bits) - 1));
        self.n += bits;
        while self.n >= 8 {
            self.n -= 8;
            self.out.push((self.acc >> self.n) as u8);
        }
        self.acc &= (1u64 << self.n) - 1;
    }

    fn put_signed(&mut self, bits: u32, v: i64) {
        self.put(bits, v as u64);
    }

    /// `q` zeros then a one.
    fn unary(&mut self, mut q: u32) {
        while q >= 32 {
            self.put(32, 0);
            q -= 32;
        }
        self.put(q + 1, 1);
    }

    fn align(&mut self) {
        if self.n > 0 {
            self.put(8 - self.n, 0);
        }
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn bytes(&self) -> &'a [u8] {
        self.data
    }

    fn byte_pos(&self) -> usize {
        self.bit / 8
    }

    fn get(&mut self, bits: u32) -> Result<u64> {
        ensure!(self.bit + bits as usize <= self.data.len() * 8, "FLAC data ends early");
        let (mut v, mut left) = (0u64, bits as usize);
        while left > 0 {
            let off = self.bit % 8;
            let take = (8 - off).min(left);
            let chunk = (self.data[self.bit / 8] >> (8 - off - take)) as u64 & ((1 << take) - 1);
            v = (v << take) | chunk;
            self.bit += take;
            left -= take;
        }
        Ok(v)
    }

    fn get_signed(&mut self, bits: u32) -> Result<i64> {
        let v = self.get(bits)?;
        let shift = 64 - bits;
        Ok(((v << shift) as i64) >> shift)
    }

    fn unary(&mut self) -> Result<u32> {
        let mut q = 0;
        loop {
            ensure!(self.bit < self.data.len() * 8, "FLAC data ends early");
            let off = self.bit % 8;
            let rest = self.data[self.bit / 8] << off;
            if rest == 0 {
                q += (8 - off) as u32;
                self.bit += 8 - off;
                continue;
            }
            let zeros = rest.leading_zeros() as usize;
            self.bit += zeros + 1;
            return Ok(q + zeros as u32);
        }
    }

    fn align(&mut self) {
        self.bit = self.bit.div_ceil(8) * 8;
    }

    fn skip_bytes(&mut self, n: usize) -> Result<()> {
        ensure!(self.bit % 8 == 0 && self.byte_pos() + n <= self.data.len(), "FLAC data ends early");
        self.bit += n * 8;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic speech-like test signal: a few drifting tones plus noise and silent gaps.
    fn signal(frames: usize, channels: u16) -> Pcm {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 401) as f64 - 200.0
        };
        let mut samples = Vec::with_capacity(frames * channels as usize);
        for i in 0..frames {
            let t = i as f64 / 16000.0;
            let gate = if (i / 6000) % 3 == 2 { 0.0 } else { 1.0 };
            for c in 0..channels {
                let v = gate
                    * (6000.0 * (t * 220.0 * std::f64::consts::TAU).sin()
                        + 2500.0 * (t * (700.0 + c as f64 * 50.0) * std::f64::consts::TAU).sin()
                        + noise());
                samples.push(v.clamp(-32768.0, 32767.0) as i16);
            }
        }
        Pcm { rate: 16000, channels, samples }
    }

    #[test]
    fn round_trips_bit_for_bit_and_is_smaller() {
        for (frames, channels) in [(16_000 * 3 + 123, 1), (4096, 1), (4097, 2), (100, 1), (300, 3)] {
            let pcm = signal(frames, channels);
            let flac = encode(&pcm).unwrap();
            assert_eq!(decode(&flac).unwrap(), pcm, "{frames} frames x {channels}");
            if frames > 1000 {
                assert!(flac.len() < pcm.samples.len() * 2 * 3 / 4, "{} vs {}", flac.len(), pcm.samples.len() * 2);
            }
        }
    }

    #[test]
    fn extremes_and_silence_survive() {
        let mut samples = vec![0i16; 5000];
        samples.extend((0..5000).map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN }));
        samples.extend((0..3000).map(|i| (i % 7) as i16 - 3));
        let pcm = Pcm { rate: 44_100, channels: 1, samples };
        assert_eq!(decode(&encode(&pcm).unwrap()).unwrap(), pcm);
        // An odd rate is not in the header table and comes from STREAMINFO instead.
        let odd = Pcm { rate: 11_111, ..pcm };
        assert_eq!(decode(&encode(&odd).unwrap()).unwrap(), odd);
    }

    #[test]
    fn wav_round_trip_is_byte_identical() {
        let wav = to_wav(&signal(20_000, 1));
        let flac = encode_wav(&wav).unwrap();
        assert_eq!(decode_to_wav(&flac).unwrap(), wav);
        // And the recorder's own WAVs parse.
        let silent = crate::audio::silent_wav(0.5).unwrap();
        assert_eq!(decode_to_wav(&encode_wav(&silent).unwrap()).unwrap(), silent);
    }

    #[test]
    fn damage_is_detected_not_decoded() {
        let mut flac = encode(&signal(10_000, 1)).unwrap();
        let i = flac.len() / 2;
        flac[i] ^= 0x10;
        assert!(decode(&flac).is_err());
        assert!(decode(&flac[..flac.len() - 3]).is_err());
        assert!(parse_wav(b"RIFF\0\0\0\0WAVE").is_err());
    }

    #[test]
    fn frame_numbers_use_the_utf8_pattern() {
        assert_eq!(utf8_number(0x41), vec![0x41]);
        assert_eq!(utf8_number(0x80), vec![0xC2, 0x80]);
        assert_eq!(utf8_number(0x20AC), vec![0xE2, 0x82, 0xAC]);
        assert_eq!(utf8_number(0x1F600), vec![0xF0, 0x9F, 0x98, 0x80]);
    }

    /// Checks the output against a real FLAC decoder when ffmpeg is installed; skipped otherwise.
    #[test]
    fn ffmpeg_decodes_it_identically() {
        if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg not installed, skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("stfu-flac-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcm = signal(16_000 * 2 + 77, 2);
        std::fs::write(dir.join("a.flac"), encode(&pcm).unwrap()).unwrap();
        let out = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-i"])
            .arg(dir.join("a.flac"))
            .args(["-f", "s16le", "-"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let decoded: Vec<i16> = out.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        assert_eq!(decoded, pcm.samples);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Manual check on real recordings: `STFU_FLAC_DIR=<folder of wavs> cargo test --release --lib
/// real_recordings -- --ignored --nocapture`. Prints the size saved and checks every round trip.
#[cfg(test)]
#[test]
#[ignore]
fn real_recordings() {
    let dir = std::env::var("STFU_FLAC_DIR").expect("set STFU_FLAC_DIR");
    let (mut wav_bytes, mut flac_bytes, mut files) = (0usize, 0usize, 0);
    let t = std::time::Instant::now();
    for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        if e.path().extension().is_none_or(|x| x != "wav") {
            continue;
        }
        let wav = std::fs::read(e.path()).unwrap();
        let flac = encode_wav(&wav).unwrap();
        assert_eq!(decode_to_wav(&flac).unwrap(), wav, "{}", e.path().display());
        wav_bytes += wav.len();
        flac_bytes += flac.len();
        files += 1;
    }
    println!(
        "{files} files: {} KB -> {} KB ({:.0}% saved) in {:?} (encode + verify)",
        wav_bytes / 1024,
        flac_bytes / 1024,
        100.0 * (1.0 - flac_bytes as f64 / wav_bytes as f64),
        t.elapsed()
    );
}
