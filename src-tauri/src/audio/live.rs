//! The live transcript shown while recording. Each side of the call is cut
//! into utterances at natural pauses, and each finished utterance is
//! transcribed once, so lines appear a moment after someone stops speaking
//! and stay put. The saved transcript is still made from the recording after
//! it stops; these lines are for following along and "Recap so far".

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::pipeline::WHISPER_RATE;
use crate::whisper::assemble;
use crate::whisper::transcribe::TranscriptSegment;

/// One line of the live transcript.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LiveLine {
    /// "mic" for the person recording's side, "system" for the call.
    pub source: &'static str,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

const FRAME: usize = (WHISPER_RATE / 1000 * 30) as usize; // 30 ms
const VOICED: f32 = 0.01;
/// A pause this long ends an utterance.
const PAUSE_FRAMES: usize = 700 / 30;
/// Long turns are cut so lines keep appearing.
const MAX_FRAMES: usize = 20_000 / 30;
/// Shorter bursts (a cough, a click) are not speech.
const MIN_VOICED_FRAMES: usize = 300 / 30;
/// Audio kept either side of an utterance, so Whisper hears it whole.
const PAD: usize = WHISPER_RATE as usize / 5;

/// Finished utterances in `pcm`, as sample ranges, and where the audio still
/// waiting for its pause begins. Everything before that can be let go.
pub fn utterances(pcm: &[f32]) -> (Vec<(usize, usize)>, usize) {
    let frames: Vec<bool> = pcm
        .chunks(FRAME)
        .map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len().max(1) as f32).sqrt() >= VOICED)
        .collect();
    let mut found = Vec::new();
    let mut start: Option<usize> = None;
    let mut last = 0;
    let mut voiced = 0;
    for (i, &is_voiced) in frames.iter().enumerate() {
        if is_voiced {
            if start.is_none() {
                start = Some(i);
                voiced = 0;
            }
            last = i;
            voiced += 1;
        }
        if let Some(s) = start {
            let paused = !is_voiced && i - last >= PAUSE_FRAMES;
            let too_long = i + 1 - s >= MAX_FRAMES;
            if paused || too_long {
                let end = if paused { last + 1 } else { i + 1 };
                if voiced >= MIN_VOICED_FRAMES {
                    found.push((s * FRAME, (end * FRAME).min(pcm.len())));
                }
                start = None;
            }
        }
    }
    let keep_from = match start {
        Some(s) => s * FRAME,
        // Nothing in progress: keep only a moment of audio for padding.
        None => pcm.len().saturating_sub(PAD),
    };
    (found, keep_from.min(pcm.len()))
}

/// One side of the call, with what has been handed on already.
#[derive(Default)]
struct Side {
    pcm: Vec<f32>,
    /// Samples dropped from the front of `pcm` so far.
    offset: usize,
}

fn ms(samples: usize) -> i64 {
    (samples as i64 * 1000) / WHISPER_RATE as i64
}

/// Transcribe utterances as they finish, until the recording stops.
pub fn spawn(
    app: AppHandle,
    stop: Arc<AtomicBool>,
    chunks: Receiver<(Vec<f32>, Vec<f32>)>,
    lines: Arc<Mutex<Vec<LiveLine>>>,
    model_path: PathBuf,
    language: String,
) -> JoinHandle<()> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    std::thread::spawn(move || {
        let ctx = match WhisperContext::new_with_params(&model_path, WhisperContextParameters::default()) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("live transcription disabled (model load failed): {e}");
                return;
            }
        };
        let mut state = match ctx.create_state() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("live transcription disabled (state failed): {e}");
                return;
            }
        };
        let threads = std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4);
        let language = if language == "auto" || language == "en" { language } else { "en".into() };
        let mut sides = [Side::default(), Side::default()];
        let mut decode = |pcm: &[f32]| -> Vec<TranscriptSegment> {
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_n_threads(threads.min(4));
            params.set_translate(false);
            params.set_language(Some(&language));
            params.set_single_segment(true);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if state.full(params, pcm).is_err() {
                return Vec::new();
            }
            state
                .as_iter()
                .filter_map(|s| s.to_str_lossy().ok().map(|t| t.trim().to_string()))
                .filter(|t| !t.is_empty())
                .map(|text| TranscriptSegment { start_ms: 0, end_ms: ms(pcm.len()), text })
                .collect()
        };

        while !stop.load(Ordering::Relaxed) {
            match chunks.recv_timeout(Duration::from_millis(300)) {
                Ok((mic, system)) => {
                    sides[0].pcm.extend(mic);
                    sides[1].pcm.extend(system);
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
            for (index, side) in sides.iter_mut().enumerate() {
                let (found, keep_from) = utterances(&side.pcm);
                for (start, end) in found {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let from = start.saturating_sub(PAD);
                    let to = (end + PAD).min(side.pcm.len());
                    let segments = assemble::keep_spoken(decode(&side.pcm[from..to]), &side.pcm[from..to]);
                    let text = segments.into_iter().map(|s| s.text).collect::<Vec<_>>().join(" ");
                    if text.is_empty() {
                        continue;
                    }
                    let line = LiveLine {
                        source: if index == 0 { "mic" } else { "system" },
                        start_ms: ms(side.offset + start),
                        end_ms: ms(side.offset + end),
                        text,
                    };
                    let Ok(mut all) = lines.lock() else { return };
                    // The call's voice coming back through the speakers.
                    if line.source == "mic" {
                        let theirs: Vec<TranscriptSegment> = all
                            .iter()
                            .filter(|l| l.source == "system" && l.end_ms + 15_000 > line.start_ms)
                            .map(|l| TranscriptSegment { start_ms: l.start_ms, end_ms: l.end_ms, text: l.text.clone() })
                            .collect();
                        let mine = TranscriptSegment { start_ms: line.start_ms, end_ms: line.end_ms, text: line.text.clone() };
                        if assemble::remove_echo(vec![mine], &theirs).is_empty() {
                            continue;
                        }
                    }
                    all.push(line.clone());
                    drop(all);
                    let _ = app.emit("live-line", line);
                }
                side.pcm.drain(..keep_from);
                side.offset += keep_from;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(seconds: f32) -> Vec<f32> {
        (0..(seconds * WHISPER_RATE as f32) as usize).map(|i| (i as f32 / 7.0).sin() * 0.2).collect()
    }
    fn quiet(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * WHISPER_RATE as f32) as usize]
    }

    #[test]
    fn utterances_end_at_pauses_and_the_rest_waits() {
        let mut pcm = speech(2.0);
        pcm.extend(quiet(1.0));
        pcm.extend(speech(1.5));
        pcm.extend(quiet(0.3)); // a short breath, not the end
        pcm.extend(speech(1.0));
        let (found, keep_from) = utterances(&pcm);
        assert_eq!(found.len(), 1, "{found:?}");
        let (start, end) = found[0];
        assert_eq!(start, 0);
        assert!((ms(end) - 2_000).abs() <= 30, "ends when speech stops: {}", ms(end));
        // The second turn has not paused yet, so it is kept for next time.
        assert!((ms(keep_from) - 3_000).abs() <= 30, "{}", ms(keep_from));
    }

    #[test]
    fn long_turns_are_cut_and_clicks_ignored() {
        let (found, _) = utterances(&speech(45.0));
        assert_eq!(found.len(), 2, "cut every 20 seconds");
        let mut click = quiet(1.0);
        click.extend(speech(0.1));
        click.extend(quiet(1.0));
        let (found, keep_from) = utterances(&click);
        assert!(found.is_empty());
        assert_eq!(keep_from, click.len() - PAD);
    }
}
