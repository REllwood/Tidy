//! Microphone capture via cpal. The returned `Stream` must be kept alive on the
//! capturing thread (cpal streams are `!Send`).

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

use super::pipeline::downmix_to_mono;
use crate::error::{AppError, AppResult};

pub struct MicStream {
    pub stream: cpal::Stream,
    pub sample_rate: u32,
}

/// Start capturing the default input device, appending mono f32 samples to `buf`.
pub fn start_mic(buf: Arc<Mutex<Vec<f32>>>) -> AppResult<MicStream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Other("no microphone input device".into()))?;
    let supported = device
        .default_input_config()
        .map_err(|e| AppError::Other(format!("mic config: {e}")))?;
    let sample_rate: u32 = supported.sample_rate();
    let channels = supported.channels() as usize;
    let config: cpal::StreamConfig = supported.clone().into();
    let err_fn = |e| log::error!("mic stream error: {e}");

    let push = move |mono: Vec<f32>| {
        if let Ok(mut b) = buf.lock() {
            b.extend_from_slice(&mono);
        }
    };

    let stream = match supported.sample_format() {
        SampleFormat::F32 => {
            let push = push.clone();
            device.build_input_stream(
                &config,
                move |data: &[f32], _| push(downmix_to_mono(data, channels)),
                err_fn,
                None,
            )
        }
        SampleFormat::I16 => {
            let push = push.clone();
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let f: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                    push(downmix_to_mono(&f, channels));
                },
                err_fn,
                None,
            )
        }
        SampleFormat::U16 => {
            let push = push.clone();
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    let f: Vec<f32> =
                        data.iter().map(|s| (*s as f32 / 32768.0) - 1.0).collect();
                    push(downmix_to_mono(&f, channels));
                },
                err_fn,
                None,
            )
        }
        other => {
            return Err(AppError::Other(format!(
                "unsupported mic sample format: {other:?}"
            )))
        }
    }
    .map_err(|e| AppError::Other(format!("build mic stream: {e}")))?;

    stream
        .play()
        .map_err(|e| AppError::Other(format!("play mic stream: {e}")))?;
    Ok(MicStream {
        stream,
        sample_rate,
    })
}
