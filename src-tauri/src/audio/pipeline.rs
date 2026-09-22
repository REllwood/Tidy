//! Audio post-processing: resample to 16 kHz mono and mix two sources.
//! Whisper wants 16 kHz mono f32 PCM. A linear resampler is more than adequate
//! for speech and keeps the dependency surface small.

pub const WHISPER_RATE: u32 = 16_000;

/// A linear-interpolation resampler that can be fed in chunks. It holds back
/// the samples a later chunk still needs to interpolate across the boundary,
/// so streaming a recording through it matches converting it in one pass.
///
/// The read position is tracked as an exact fraction of the source rate rather
/// than an accumulating float, so chunking cannot drift a sample away from the
/// single-pass result.
pub struct Resampler {
    src_rate: u64,
    dst_rate: u64,
    /// Source index of the next output sample, counted from the stream start.
    whole: u64,
    /// The fraction of the way past it, over `dst_rate`.
    remainder: u64,
    /// Source samples already dropped from the front of `tail`.
    consumed: u64,
    tail: Vec<f32>,
}

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            src_rate: src_rate as u64,
            dst_rate: dst_rate as u64,
            whole: 0,
            remainder: 0,
            consumed: 0,
            tail: Vec::new(),
        }
    }

    fn usable(&self) -> bool {
        self.src_rate != 0 && self.dst_rate != 0 && self.src_rate != self.dst_rate
    }

    fn index(&self) -> u64 {
        self.whole - self.consumed
    }

    fn interpolate(&self) -> f32 {
        let index = self.index() as usize;
        let fraction = (self.remainder as f64 / self.dst_rate as f64) as f32;
        let a = self.tail.get(index).copied().unwrap_or(0.0);
        let b = self.tail.get(index + 1).copied().unwrap_or(a);
        a + (b - a) * fraction
    }

    fn advance(&mut self) {
        self.remainder += self.src_rate;
        self.whole += self.remainder / self.dst_rate;
        self.remainder %= self.dst_rate;
    }

    /// Convert the next chunk. Samples that need the following chunk to be
    /// interpolated are carried over rather than guessed at.
    pub fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        if !self.usable() {
            return if self.src_rate == self.dst_rate && self.src_rate != 0 {
                samples.to_vec()
            } else {
                Vec::new()
            };
        }
        self.tail.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.index() + 1 < self.tail.len() as u64 {
            out.push(self.interpolate());
            self.advance();
        }
        // Release the source samples no future output can refer to.
        let drop = self.index().min(self.tail.len() as u64);
        if drop > 0 {
            self.tail.drain(..drop as usize);
            self.consumed += drop;
        }
        out
    }

    /// Emit the samples that were waiting on the end of the stream.
    pub fn finish(&mut self) -> Vec<f32> {
        if !self.usable() {
            self.tail.clear();
            return Vec::new();
        }
        let mut out = Vec::new();
        while self.index() < self.tail.len() as u64 {
            out.push(self.interpolate());
            self.advance();
        }
        self.tail.clear();
        self.whole = 0;
        self.remainder = 0;
        self.consumed = 0;
        out
    }
}

/// Linear-interpolation resample of a mono f32 stream to `dst_rate`.
pub fn resample_mono(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == 0 || dst_rate == 0 || samples.is_empty() {
        return Vec::new();
    }
    if src_rate == dst_rate {
        return samples.to_vec();
    }
    let mut resampler = Resampler::new(src_rate, dst_rate);
    let mut out = resampler.push(samples);
    out.extend(resampler.finish());
    out
}

/// Downmix interleaved frames to mono by averaging channels.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Soft limiter. Normal levels pass through untouched and only the peaks that
/// would clip are compressed, so one loud moment cannot quieten a whole
/// recording the way scaling by the global peak does.
pub fn soft_limit(sample: f32) -> f32 {
    const THRESHOLD: f32 = 0.8;
    if !sample.is_finite() {
        return 0.0;
    }
    let magnitude = sample.abs();
    if magnitude <= THRESHOLD {
        return sample;
    }
    let over = magnitude - THRESHOLD;
    let headroom = 1.0 - THRESHOLD;
    sample.signum() * (THRESHOLD + headroom * over / (over + headroom))
}

/// Mix two mono streams (already at the same rate). Pads the shorter with
/// silence and limits peaks so the mix stays inside the sample range.
pub fn mix_mono(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| soft_limit(a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0)))
        .collect()
}

/// A recording's two live sources, mixed to 16 kHz mono a chunk at a time.
/// Whatever has no counterpart yet is held back, so the microphone and the
/// system capture stay aligned in time across the whole meeting.
pub struct ChunkedMixer {
    mic: Resampler,
    system: Resampler,
    mic_pending: Vec<f32>,
    system_pending: Vec<f32>,
    mic_present: bool,
    system_present: bool,
}

/// How far one source may fall behind the other before the recording carries on
/// without it. A source that stops delivering must not stall the whole meeting.
const MAX_SKEW: usize = WHISPER_RATE as usize * 5;

impl ChunkedMixer {
    pub fn new(mic_rate: u32, mic_present: bool, system_rate: u32, system_present: bool) -> Self {
        Self {
            mic: Resampler::new(mic_rate, WHISPER_RATE),
            system: Resampler::new(system_rate, WHISPER_RATE),
            mic_pending: Vec::new(),
            system_pending: Vec::new(),
            mic_present,
            system_present,
        }
    }

    /// Add newly captured samples and return whatever is ready to be written.
    pub fn push(&mut self, mic: &[f32], system: &[f32]) -> Vec<f32> {
        let converted = self.mic.push(mic);
        self.mic_pending.extend(converted);
        let converted = self.system.push(system);
        self.system_pending.extend(converted);
        let ready = match (self.mic_present, self.system_present) {
            (true, true) => {
                let overlap = self.mic_pending.len().min(self.system_pending.len());
                let longest = self.mic_pending.len().max(self.system_pending.len());
                overlap.max(longest.saturating_sub(MAX_SKEW))
            }
            (true, false) => self.mic_pending.len(),
            (false, true) => self.system_pending.len(),
            (false, false) => 0,
        };
        self.take(ready)
    }

    /// Mix everything that is left once capture has stopped.
    pub fn flush(&mut self) -> Vec<f32> {
        let converted = self.mic.finish();
        self.mic_pending.extend(converted);
        let converted = self.system.finish();
        self.system_pending.extend(converted);
        let remaining = self.mic_pending.len().max(self.system_pending.len());
        self.take(remaining)
    }

    fn take(&mut self, n: usize) -> Vec<f32> {
        if n == 0 {
            return Vec::new();
        }
        let mic: Vec<f32> = self
            .mic_pending
            .drain(..n.min(self.mic_pending.len()))
            .collect();
        let system: Vec<f32> = self
            .system_pending
            .drain(..n.min(self.system_pending.len()))
            .collect();
        mix_mono(&mic, &system)
    }
}

/// RMS level (0..1) of a buffer — used to drive the input level meters.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Combine two raw captured sources into a single 16 kHz mono buffer in one
/// pass. Recording streams its audio through `ChunkedMixer` instead; this is
/// the reference the chunked path is measured against.
#[cfg(test)]
pub fn combine_sources(mic: &[f32], mic_rate: u32, system: &[f32], system_rate: u32) -> Vec<f32> {
    let mic16 = resample_mono(mic, mic_rate, WHISPER_RATE);
    let sys16 = resample_mono(system, system_rate, WHISPER_RATE);
    mix_mono(&mic16, &sys16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(samples: usize, rate: u32, hz: f32, amplitude: f32) -> Vec<f32> {
        (0..samples)
            .map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * amplitude)
            .collect()
    }

    // Deterministic pseudo-noise: a real generator would make the comparison
    // between the incremental and one-shot paths impossible to reproduce.
    fn noise(samples: usize, amplitude: f32) -> Vec<f32> {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        (0..samples)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (((state >> 40) & 0xFFF) as f32 / 4096.0 - 0.5) * 2.0 * amplitude
            })
            .collect()
    }

    #[test]
    fn resample_changes_length_by_ratio() {
        let src: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = resample_mono(&src, 48_000, 16_000);
        // ~1/3 the samples
        assert!((out.len() as i64 - 16_000).abs() < 5, "len={}", out.len());
    }

    #[test]
    fn resample_identity() {
        let src = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_mono(&src, 16_000, 16_000), src);
    }

    #[test]
    fn chunked_resampling_matches_one_pass() {
        let src = sine(44_100, 44_100, 440.0, 0.6);
        let expected = resample_mono(&src, 44_100, WHISPER_RATE);
        let mut resampler = Resampler::new(44_100, WHISPER_RATE);
        let mut out = Vec::new();
        for chunk in src.chunks(4_097) {
            out.extend(resampler.push(chunk));
        }
        out.extend(resampler.finish());
        assert_eq!(out.len(), expected.len());
        assert_eq!(out, expected);
    }

    #[test]
    fn downmix_averages_channels() {
        let stereo = vec![1.0, 0.0, 0.5, 0.5];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.5, 0.5]);
    }

    #[test]
    fn mix_limits_peaks_without_quietening_the_rest() {
        let a = vec![0.8, 0.8, 0.8];
        let b = vec![0.8]; // shorter
        let m = mix_mono(&a, &b);
        assert_eq!(m.len(), 3);
        assert!(m[0] > 0.8 && m[0] < 1.0, "peak limited, not clipped: {}", m[0]);
        // A loud moment must not scale the quieter part down.
        assert_eq!(m[1], 0.8);
        assert_eq!(m[2], 0.8);
        assert_eq!(soft_limit(-2.0), -soft_limit(2.0));
        assert!(soft_limit(1_000.0) <= 1.0);
    }

    #[test]
    fn combine_two_sources_to_16k() {
        let mic: Vec<f32> = vec![0.2; 44_100];
        let sys: Vec<f32> = vec![0.1; 48_000];
        let out = combine_sources(&mic, 44_100, &sys, 48_000);
        // both ~1s → ~16000 samples
        assert!((out.len() as i64 - 16_000).abs() < 50);
        assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn incremental_mix_matches_a_single_pass_and_never_clips() {
        // Loud enough that a naive sum would clip well past full scale.
        let mic = sine(44_100 * 3, 44_100, 300.0, 0.9);
        let system = noise(48_000 * 3, 0.9);
        let expected = combine_sources(&mic, 44_100, &system, 48_000);

        let mut mixer = ChunkedMixer::new(44_100, true, 48_000, true);
        let mut out = Vec::new();
        // Five-second chunks in real time land as uneven, interleaved arrivals.
        for (m, s) in mic.chunks(44_100 / 2).zip(system.chunks(48_000 / 2)) {
            out.extend(mixer.push(m, s));
        }
        out.extend(mixer.flush());

        assert_eq!(out.len(), expected.len());
        assert_eq!(out.len(), 16_000 * 3);
        assert_eq!(out, expected);
        assert!(out.iter().all(|s| s.abs() <= 1.0));
        assert!(out.iter().any(|s| s.abs() > 0.8), "the mix must stay loud");
    }

    #[test]
    fn a_single_source_is_written_without_waiting_for_the_other() {
        let mic = sine(16_000, 16_000, 220.0, 0.5);
        let mut mixer = ChunkedMixer::new(16_000, true, 48_000, false);
        let mut out = Vec::new();
        for chunk in mic.chunks(1_000) {
            out.extend(mixer.push(chunk, &[]));
        }
        assert_eq!(out.len(), mic.len(), "nothing is held back for a dead source");
        out.extend(mixer.flush());
        assert_eq!(out, mic);
    }

    #[test]
    fn a_stalled_source_does_not_hold_up_the_recording() {
        let mic = sine(WHISPER_RATE as usize * 9, WHISPER_RATE, 200.0, 0.4);
        let mut mixer = ChunkedMixer::new(WHISPER_RATE, true, WHISPER_RATE, true);
        let written = mixer.push(&mic, &[]);
        assert!(
            written.len() >= WHISPER_RATE as usize * 3,
            "written={}",
            written.len()
        );
        assert!(written.len() <= mic.len());
    }

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 100]), 0.0);
    }
}
