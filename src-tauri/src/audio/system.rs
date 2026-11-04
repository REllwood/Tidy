//! System (output) audio capture. A Core Audio tap is used when it can be,
//! since it needs only permission to record system audio. ScreenCaptureKit,
//! which needs Screen Recording permission, is kept for people who already
//! granted it and as the fallback. The capture must be kept alive on the
//! capturing thread.

use std::sync::{Arc, Mutex};

use screencapturekit::prelude::*;

use super::pipeline::downmix_to_mono;
use crate::error::{AppError, AppResult};

pub const SYSTEM_RATE: u32 = 48_000;
/// The channel count we ask ScreenCaptureKit for below.
pub const SYSTEM_CHANNELS: usize = 2;

struct AudioHandler {
    buf: Arc<Mutex<Vec<f32>>>,
}

fn buffer_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Average a set of planar channel buffers element-wise.
fn average_planar(channels: &[Vec<f32>]) -> Vec<f32> {
    let len = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let denom = channels.len() as f32;
    (0..len)
        .map(|i| channels.iter().map(|c| c[i]).sum::<f32>() / denom)
        .collect()
}

impl SCStreamOutputTrait for AudioHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }
        let Some(list) = sample.audio_buffer_list() else {
            return;
        };
        let n = list.num_buffers();
        if n == 0 {
            return;
        }
        // ScreenCaptureKit usually delivers planar float, one buffer per
        // channel, but a single buffer holding interleaved frames is just as
        // valid. Treating interleaved data as planar halves the sample rate and
        // turns speech into a chipmunk, so branch on what actually arrived
        // instead of assuming.
        if n != SYSTEM_CHANNELS && n != 1 {
            log::warn!("system audio delivered {n} buffers for {SYSTEM_CHANNELS} channels");
        }
        let mono = if n == 1 {
            let Some(buffer) = list.get(0) else {
                return;
            };
            let channels = (buffer.number_channels as usize).max(1);
            let samples = buffer_to_f32(buffer.data());
            if channels > 1 {
                downmix_to_mono(&samples, channels)
            } else {
                samples
            }
        } else {
            let channels: Vec<Vec<f32>> = (0..n)
                .filter_map(|i| {
                    list.get(i).map(|buffer| {
                        let samples = buffer_to_f32(buffer.data());
                        let count = (buffer.number_channels as usize).max(1);
                        if count > 1 {
                            downmix_to_mono(&samples, count)
                        } else {
                            samples
                        }
                    })
                })
                .collect();
            if channels.is_empty() {
                return;
            }
            average_planar(&channels)
        };
        if mono.is_empty() {
            return;
        }
        if let Ok(mut b) = self.buf.lock() {
            b.extend_from_slice(&mono);
        }
    }
}

/// A running system audio capture. Dropping it stops capture.
pub enum SystemCapture {
    /// Held only so capture runs until it is dropped.
    Screen { _stream: SCStream },
    Tap(super::tap::TapCapture),
}

impl SystemCapture {
    pub fn sample_rate(&self) -> u32 {
        match self {
            SystemCapture::Screen { .. } => SYSTEM_RATE,
            SystemCapture::Tap(tap) => tap.sample_rate,
        }
    }

    /// A tap that is not allowed to record delivers silence rather than an
    /// error, so silence from a tap is worth a hint.
    pub fn is_tap(&self) -> bool {
        matches!(self, SystemCapture::Tap(_))
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// Whether Screen Recording is already allowed, checked without asking.
fn screen_capture_allowed() -> bool {
    // SAFETY: a side-effect-free query.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Start system-audio capture, appending mono f32 samples (at the capture's
/// `sample_rate`) to `buf`.
pub fn start_system(buf: Arc<Mutex<Vec<f32>>>) -> AppResult<SystemCapture> {
    // Already allowed to record the screen: keep the capture that works.
    if screen_capture_allowed() {
        return start_screen(buf).map(|_stream| SystemCapture::Screen { _stream });
    }
    match super::tap::start_tap(buf.clone()) {
        Ok(tap) => Ok(SystemCapture::Tap(tap)),
        Err(error) => {
            log::warn!("Core Audio tap unavailable, using ScreenCaptureKit: {error}");
            start_screen(buf).map(|_stream| SystemCapture::Screen { _stream })
        }
    }
}

/// ScreenCaptureKit capture of a display's audio, as mono at `SYSTEM_RATE`.
fn start_screen(buf: Arc<Mutex<Vec<f32>>>) -> AppResult<SCStream> {
    let content = SCShareableContent::get()
        .map_err(|e| AppError::Other(format!("shareable content: {e}")))?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Other("no display available for audio capture".into()))?;

    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build();
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_sample_rate(SYSTEM_RATE as i32)
        .with_channel_count(SYSTEM_CHANNELS as i32);

    let mut stream = SCStream::new(&filter, &config);
    stream.add_output_handler(AudioHandler { buf }, SCStreamOutputType::Audio);
    stream
        .start_capture()
        .map_err(|e| AppError::Other(format!("start system audio capture: {e}")))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_and_planar_stereo_downmix_to_the_same_mono() {
        // Left and right channels of two frames.
        let interleaved = vec![1.0, 0.0, 0.5, 0.5];
        let planar = vec![vec![1.0, 0.5], vec![0.0, 0.5]];
        assert_eq!(downmix_to_mono(&interleaved, SYSTEM_CHANNELS), vec![0.5, 0.5]);
        assert_eq!(average_planar(&planar), vec![0.5, 0.5]);
    }

    #[test]
    fn planar_buffers_of_different_lengths_use_the_shorter_one() {
        let planar = vec![vec![1.0, 1.0, 1.0], vec![0.0]];
        assert_eq!(average_planar(&planar), vec![0.5]);
    }

    #[test]
    fn bytes_decode_as_little_endian_floats() {
        let mut bytes = Vec::new();
        for value in [0.25f32, -0.5] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0); // a trailing partial sample is ignored
        assert_eq!(buffer_to_f32(&bytes), vec![0.25, -0.5]);
    }
}
