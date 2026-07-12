//! Audio post-processing: resample to 16 kHz mono and mix two sources.
//! Whisper wants 16 kHz mono f32 PCM. A linear resampler is more than adequate
//! for speech and keeps the dependency surface small.

pub const WHISPER_RATE: u32 = 16_000;

/// Linear-interpolation resample of a mono f32 stream to `dst_rate`.
pub fn resample_mono(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == 0 || dst_rate == 0 || samples.is_empty() {
        return Vec::new();
    }
    if src_rate == dst_rate {
        return samples.to_vec();
    }
    let ratio = dst_rate as f64 / src_rate as f64;
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = samples.get(idx).copied().unwrap_or(0.0);
        let b = samples.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
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

/// Mix two mono streams (already at the same rate). Pads the shorter with
/// silence and soft-clamps the sum to [-1, 1].
pub fn mix_mono(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let s = a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0);
        out.push(s.clamp(-1.0, 1.0));
    }
    out
}

/// RMS level (0..1) of a buffer — used to drive the input level meters.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Combine two raw captured sources into a single 16 kHz mono buffer.
pub fn combine_sources(
    mic: &[f32],
    mic_rate: u32,
    system: &[f32],
    system_rate: u32,
) -> Vec<f32> {
    let mic16 = resample_mono(mic, mic_rate, WHISPER_RATE);
    let sys16 = resample_mono(system, system_rate, WHISPER_RATE);
    mix_mono(&mic16, &sys16)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn downmix_averages_channels() {
        let stereo = vec![1.0, 0.0, 0.5, 0.5];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.5, 0.5]);
    }

    #[test]
    fn mix_pads_and_clamps() {
        let a = vec![0.8, 0.8, 0.8];
        let b = vec![0.8]; // shorter
        let m = mix_mono(&a, &b);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0], 1.0); // 1.6 clamped
        assert_eq!(m[1], 0.8); // padded with silence
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
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 100]), 0.0);
    }
}
