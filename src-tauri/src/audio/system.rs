//! System (output) audio capture via ScreenCaptureKit. We capture a display's
//! audio only (no meaningful video) and downmix to mono. The returned `SCStream`
//! must be kept alive on the capturing thread.

use std::sync::{Arc, Mutex};

use screencapturekit::prelude::*;

use crate::error::{AppError, AppResult};

pub const SYSTEM_RATE: u32 = 48_000;

struct AudioHandler {
    buf: Arc<Mutex<Vec<f32>>>,
}

fn buffer_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
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
        // ScreenCaptureKit delivers planar float (one buffer per channel).
        // Average channels element-wise to mono. (A single buffer = already mono.)
        let channels: Vec<Vec<f32>> = (0..n)
            .filter_map(|i| list.buffer(i).map(|b| buffer_to_f32(b.data())))
            .collect();
        if channels.is_empty() {
            return;
        }
        let len = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let denom = channels.len() as f32;
        let mut mono = Vec::with_capacity(len);
        for i in 0..len {
            let s: f32 = channels.iter().map(|c| c[i]).sum();
            mono.push(s / denom);
        }
        if let Ok(mut b) = self.buf.lock() {
            b.extend_from_slice(&mono);
        }
    }
}

/// Start system-audio capture, appending mono f32 samples (at `SYSTEM_RATE`) to `buf`.
pub fn start_system(buf: Arc<Mutex<Vec<f32>>>) -> AppResult<SCStream> {
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
        .with_channel_count(2);

    let mut stream = SCStream::new(&filter, &config);
    stream.add_output_handler(AudioHandler { buf }, SCStreamOutputType::Audio);
    stream
        .start_capture()
        .map_err(|e| AppError::Other(format!("start system audio capture: {e}")))?;
    Ok(stream)
}
