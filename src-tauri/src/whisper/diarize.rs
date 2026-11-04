//! On-device speaker diarization via sherpa-onnx (pyannote segmentation + a
//! speaker-embedding model + fast clustering). Models live in
//! `app_data/models/diarization/{segmentation.onnx, embedding.onnx}`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::audio::pipeline::WHISPER_RATE;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::whisper::assemble::Turn;
use crate::whisper::transcribe::load_pcm_16k_mono;

#[derive(Debug, Serialize, Clone)]
pub struct SpeakerSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: i32,
}

pub fn diarization_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = crate::workspace::directory(app)
        .join("models")
        .join("diarization");
    Ok(dir)
}

fn model_paths(app: &AppHandle) -> AppResult<(PathBuf, PathBuf)> {
    let dir = diarization_dir(app)?;
    let seg = dir.join("segmentation.onnx");
    let emb = dir.join("embedding.onnx");
    if !seg.exists() || !emb.exists() {
        return Err(AppError::Invalid("diarization models not installed".into()));
    }
    Ok((seg, emb))
}

pub fn models_present(app: &AppHandle) -> bool {
    model_paths(app).is_ok()
}

/// Run diarization over a 16 kHz mono WAV. `num_speakers` <= 0 means auto-detect.
pub fn diarize_file(
    app: &AppHandle,
    wav: &std::path::Path,
    num_speakers: i32,
) -> AppResult<Vec<SpeakerSegment>> {
    let pcm = load_pcm_16k_mono(wav)?;
    diarize_pcm(app, &pcm, num_speakers)
}

/// Run diarization over 16 kHz mono samples, such as one side of a call.
pub fn diarize_pcm(
    app: &AppHandle,
    pcm: &[f32],
    num_speakers: i32,
) -> AppResult<Vec<SpeakerSegment>> {
    let (seg, emb) = model_paths(app)?;
    diarize_with(&seg, &emb, pcm, num_speakers)
}

fn diarize_with(
    seg: &Path,
    emb: &Path,
    pcm: &[f32],
    num_speakers: i32,
) -> AppResult<Vec<SpeakerSegment>> {
    use sherpa_onnx::{
        FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    };

    if pcm.is_empty() {
        return Ok(vec![]);
    }

    let mut config = OfflineSpeakerDiarizationConfig::default();
    config.segmentation.pyannote.model = Some(seg.to_string_lossy().to_string());
    config.embedding.model = Some(emb.to_string_lossy().to_string());
    config.segmentation.num_threads = threads();
    config.embedding.num_threads = threads();
    config.clustering = FastClusteringConfig {
        num_clusters: if num_speakers > 0 { num_speakers } else { -1 },
        threshold: 0.5,
    };

    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| AppError::Other("failed to init diarizer".into()))?;
    let result = diarizer
        .process(pcm)
        .ok_or_else(|| AppError::Other("diarization produced no result".into()))?;

    Ok(result
        .sort_by_start_time()
        .into_iter()
        .map(|s| SpeakerSegment {
            start_ms: (s.start * 1000.0) as i64,
            end_ms: (s.end * 1000.0) as i64,
            speaker: s.speaker,
        })
        .collect())
}

/// Speaker turns for one channel, in the shape transcript assembly uses.
/// `exact` is how many people the person said there are (0 when unknown),
/// and `at_most` how many could be there, such as everyone invited.
pub fn turns(app: &AppHandle, pcm: &[f32], exact: i32, at_most: i32) -> AppResult<Vec<Turn>> {
    let (seg, emb) = model_paths(app)?;
    voices_in(&seg, &emb, pcm, exact, at_most)
}

fn voices_in(seg: &Path, emb: &Path, pcm: &[f32], exact: i32, at_most: i32) -> AppResult<Vec<Turn>> {
    let found: Vec<Turn> = diarize_with(seg, emb, pcm, exact.max(-1))?
        .into_iter()
        .map(|s| Turn { start_ms: s.start_ms, end_ms: s.end_ms, speaker: s.speaker })
        .collect();
    if exact > 0 {
        return Ok(found);
    }
    rejoin(emb, pcm, found, at_most.max(0) as usize)
}

// The first clustering only joins two stretches of speech when every pair in
// them sounds alike. Over a long call one person's voice drifts (a laugh, a
// bad line, talking over someone), so a single person comes back as several
// voices and four people can become twenty. Each voice is heard again as a
// whole and voices that sound like one person are joined.

/// Two voices are one person when they sound as alike to each other as each
/// sounds to itself, give or take this much.
const MARGIN: f32 = 0.08;
/// Voices less alike than this are never one person, however unsteady.
const FLOOR: f32 = 0.5;
/// A minor voice joins the voice it sounds most like when this alike.
const LIKELY_SAME: f32 = 0.4;
/// A voice with under this share of the speech is minor.
const MINOR_SHARE: f64 = 0.03;
/// Less speech than this is a fragment (a cough, a laugh, two people at
/// once), not a person. It joins whoever it sounds most like.
const FRAGMENT_MS: i64 = 4_000;
/// How much of each voice is listened to, in pieces this long.
const HEARD_MS: i64 = 60_000;
const PIECE_MS: i64 = 6_000;
/// Too little audio to tell what a voice sounds like.
const TOO_SHORT_MS: i64 = 1_500;

/// One voice the first clustering found.
#[derive(Debug, Clone)]
pub struct Voice {
    pub speech_ms: i64,
    /// Speaker embeddings of pieces of its speech. Empty when there was too
    /// little audio to tell what it sounds like.
    pub pieces: Vec<Vec<f32>>,
}

fn threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get().min(4) as i32).unwrap_or(2)
}

fn samples(ms: i64) -> usize {
    ms.max(0) as usize * WHISPER_RATE as usize / 1000
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let size = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (size(a) * size(b)).max(f32::EPSILON)
}

/// How alike two sets of pieces sound, on average.
fn alike(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    let total: f32 = a.iter().flat_map(|x| b.iter().map(move |y| cosine(x, y))).sum();
    total / (a.len() * b.len()).max(1) as f32
}

/// How alike a voice's own pieces sound to each other, when it has two or more.
fn steadiness(pieces: &[Vec<f32>]) -> Option<f32> {
    let n = pieces.len();
    (n >= 2).then(|| {
        let mut total = 0.0;
        for i in 0..n {
            for j in i + 1..n {
                total += cosine(&pieces[i], &pieces[j]);
            }
        }
        total / (n * (n - 1) / 2) as f32
    })
}

/// For each voice, the voice it ends up part of (an index into `voices`).
/// Voices that sound like one person are joined, the most alike first, and
/// then more until there are no more than `at_most` (0 means no limit).
/// Voices without pieces are left as they are.
pub fn join_voices(voices: &[Voice], at_most: usize) -> Vec<usize> {
    struct Group {
        members: Vec<usize>,
        speech_ms: i64,
        pieces: Vec<Vec<f32>>,
        steady: Option<f32>,
    }
    let total = voices.iter().map(|v| v.speech_ms).sum::<i64>().max(1) as f64;
    let mut groups: Vec<Group> = voices
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.pieces.is_empty())
        .map(|(i, v)| Group { members: vec![i], speech_ms: v.speech_ms, pieces: v.pieces.clone(), steady: steadiness(&v.pieces) })
        .collect();
    let minor = |g: &Group| (g.speech_ms as f64) < total * MINOR_SHARE;
    loop {
        let too_many = at_most > 0 && groups.len() > at_most;
        let mut best: Option<(usize, usize, f32)> = None;
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                let (a, b) = (&groups[i], &groups[j]);
                let close = alike(&a.pieces, &b.pieces);
                // A voice heard in one piece has nothing to measure itself by.
                let itself = match (a.steady, b.steady) {
                    (Some(x), Some(y)) => x.min(y),
                    (Some(x), None) | (None, Some(x)) => x,
                    (None, None) => 1.0,
                };
                let fragment = a.speech_ms < FRAGMENT_MS || b.speech_ms < FRAGMENT_MS;
                let join = too_many
                    || fragment
                    || (close >= FLOOR && close >= itself - MARGIN)
                    || (close >= LIKELY_SAME && (minor(a) || minor(b)));
                if join && best.map_or(true, |(_, _, most)| close > most) {
                    best = Some((i, j, close));
                }
            }
        }
        let Some((i, j, _)) = best else { break };
        let joined = groups.remove(j);
        let into = &mut groups[i];
        into.members.extend(joined.members);
        into.speech_ms += joined.speech_ms;
        into.pieces.extend(joined.pieces);
        into.steady = steadiness(&into.pieces);
    }
    let mut part_of: Vec<usize> = (0..voices.len()).collect();
    for group in groups {
        let first = *group.members.iter().min().unwrap_or(&0);
        for member in group.members {
            part_of[member] = first;
        }
    }
    part_of
}

/// What a voice sounds like: speaker embeddings of pieces of up to a minute
/// of its longest turns.
fn pieces_of(extractor: &sherpa_onnx::SpeakerEmbeddingExtractor, pcm: &[f32], turns: &[&Turn]) -> Vec<Vec<f32>> {
    let at = |ms: i64| samples(ms).min(pcm.len());
    let mut longest: Vec<&&Turn> = turns.iter().collect();
    longest.sort_by_key(|t| std::cmp::Reverse(t.end_ms - t.start_ms));
    // Turn edges can hold a moment of the person before or after.
    let mut speech: Vec<f32> = Vec::new();
    for turn in longest {
        let trim = if turn.end_ms - turn.start_ms > 1_000 { 150 } else { 0 };
        speech.extend_from_slice(&pcm[at(turn.start_ms + trim)..at(turn.end_ms - trim)]);
        if speech.len() >= samples(HEARD_MS) {
            break;
        }
    }
    let too_short = samples(TOO_SHORT_MS);
    speech
        .chunks(samples(PIECE_MS))
        .filter(|p| p.len() >= too_short)
        .filter_map(|part| {
            let stream = extractor.create_stream()?;
            stream.accept_waveform(WHISPER_RATE as i32, part);
            stream.input_finished();
            if !extractor.is_ready(&stream) {
                return None;
            }
            extractor.compute(&stream)
        })
        .collect()
}

/// Turns of voices too short to hear take the voice speaking nearest to them.
fn fold_unheard(turns: &mut [Turn], heard: &HashSet<i32>) {
    let known: Vec<Turn> = turns.iter().filter(|t| heard.contains(&t.speaker)).cloned().collect();
    for turn in turns.iter_mut().filter(|t| !heard.contains(&t.speaker)) {
        let gap = |k: &Turn| (k.start_ms - turn.end_ms).max(turn.start_ms - k.end_ms).max(0);
        if let Some(nearest) = known.iter().min_by_key(|k| gap(k)) {
            turn.speaker = nearest.speaker;
        }
    }
}

/// Join voices the first clustering split from one person, then join turns
/// of the same voice that now touch.
fn rejoin(emb: &Path, pcm: &[f32], mut turns: Vec<Turn>, at_most: usize) -> AppResult<Vec<Turn>> {
    use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig};

    let mut labels: Vec<i32> = Vec::new();
    for turn in &turns {
        if !labels.contains(&turn.speaker) {
            labels.push(turn.speaker);
        }
    }
    if labels.len() < 2 {
        return Ok(turns);
    }
    let config = SpeakerEmbeddingExtractorConfig {
        model: Some(emb.to_string_lossy().to_string()),
        num_threads: threads(),
        ..Default::default()
    };
    let extractor = SpeakerEmbeddingExtractor::create(&config)
        .ok_or_else(|| AppError::Other("failed to load the speaker model".into()))?;
    let sounds: Vec<Vec<Vec<f32>>> = labels
        .iter()
        .map(|label| {
            let theirs: Vec<&Turn> = turns.iter().filter(|t| t.speaker == *label).collect();
            pieces_of(&extractor, pcm, &theirs)
        })
        .collect();
    let heard: HashSet<i32> = labels.iter().zip(&sounds).filter(|(_, s)| !s.is_empty()).map(|(l, _)| *l).collect();
    if heard.is_empty() {
        return Ok(turns);
    }
    fold_unheard(&mut turns, &heard);
    let voices: Vec<Voice> = labels
        .iter()
        .zip(sounds)
        .map(|(label, pieces)| Voice {
            speech_ms: turns.iter().filter(|t| t.speaker == *label).map(|t| t.end_ms - t.start_ms).sum(),
            pieces,
        })
        .collect();
    let part_of = join_voices(&voices, at_most);
    for turn in &mut turns {
        if let Some(index) = labels.iter().position(|l| *l == turn.speaker) {
            turn.speaker = labels[part_of[index]];
        }
    }
    log::info!(
        "Speakers: {} voices found, {} after joining",
        labels.len(),
        turns.iter().map(|t| t.speaker).collect::<HashSet<_>>().len()
    );
    Ok(join_touching(turns))
}

/// One voice's turns that now run into each other become one turn.
fn join_touching(mut turns: Vec<Turn>) -> Vec<Turn> {
    turns.sort_by_key(|t| (t.start_ms, t.end_ms));
    let mut out: Vec<Turn> = Vec::with_capacity(turns.len());
    for turn in turns {
        match out.last_mut() {
            Some(last) if last.speaker == turn.speaker && turn.start_ms <= last.end_ms + 500 => {
                last.end_ms = last.end_ms.max(turn.end_ms);
            }
            _ => out.push(turn),
        }
    }
    out
}

#[tauri::command]
pub async fn diarize(
    app: AppHandle,
    _db: State<'_, Db>,
    audio_path: String,
    num_speakers: i32,
) -> AppResult<Vec<SpeakerSegment>> {
    // Validate the path is inside our recordings dir (same guard as transcribe).
    let audio = std::path::PathBuf::from(&audio_path);
    let recordings = crate::audio::recorder::app_recordings_dir(&app)?;
    let canon_audio = audio
        .canonicalize()
        .map_err(|_| AppError::Invalid("recording not found".into()))?;
    let canon_dir = recordings
        .canonicalize()
        .map_err(|_| AppError::Invalid("recording not found".into()))?;
    if !canon_audio.starts_with(&canon_dir) {
        return Err(AppError::Invalid("recording not found".into()));
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || diarize_file(&app2, &canon_audio, num_speakers))
        .await
        .map_err(|e| AppError::Other(format!("diarize task: {e}")))?
}

#[tauri::command]
pub fn diarization_available(app: AppHandle) -> bool {
    models_present(&app)
}

const SEG_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
const EMB_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx";

/// Download the diarization models (segmentation .tar.bz2 + embedding .onnx).
#[tauri::command]
pub async fn download_diarization_models(app: AppHandle) -> AppResult<()> {
    use tauri::Emitter;
    let dir = diarization_dir(&app)?;
    tokio::fs::create_dir_all(&dir).await?;
    let client = reqwest::Client::builder()
        .user_agent("Tidy/0.1")
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;

    // embedding model (direct .onnx)
    let emb = client
        .get(EMB_URL)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("embedding download: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Other(format!("embedding status: {e}")))?
        .bytes()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    tokio::fs::write(dir.join("embedding.onnx"), &emb).await?;
    let _ = app.emit("diarization-download-progress", "embedding");

    // segmentation model (tar.bz2 → extract model.onnx → segmentation.onnx)
    let archive = client
        .get(SEG_URL)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("segmentation download: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Other(format!("segmentation status: {e}")))?
        .bytes()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
        .to_vec();
    let dest = dir.join("segmentation.onnx");
    tauri::async_runtime::spawn_blocking(move || extract_segmentation(&archive, &dest))
        .await
        .map_err(|e| AppError::Other(format!("extract task: {e}")))??;
    let _ = app.emit("diarization-download-progress", "done");
    Ok(())
}

fn extract_segmentation(archive: &[u8], dest: &std::path::Path) -> AppResult<()> {
    let decoder = bzip2_rs::DecoderReader::new(archive);
    let mut ar = tar::Archive::new(decoder);
    for entry in ar.entries()? {
        let mut e = entry?;
        let path = e.path()?.into_owned();
        if path.file_name().and_then(|n| n.to_str()) == Some("model.onnx") {
            use std::io::Read;
            let mut out = std::fs::File::create(dest)?;
            // cap extraction size (defense in depth; the real model is ~6 MB)
            const MAX: u64 = 200 * 1024 * 1024;
            std::io::copy(&mut e.by_ref().take(MAX), &mut out)?;
            return Ok(());
        }
    }
    Err(AppError::Other(
        "model.onnx not found in segmentation archive".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A voice heard in three pieces that are `steady` alike to each other,
    /// all near `sound`.
    fn unsteady(seconds: i64, sound: &[f32], steady: f32) -> Voice {
        let off = (1.0 / steady - 1.0).sqrt();
        let pieces = (0..3)
            .map(|k| {
                let mut piece = sound.to_vec();
                piece.extend([0.0; 3]);
                piece[sound.len() + k] = off;
                piece
            })
            .collect();
        Voice { speech_ms: seconds * 1000, pieces }
    }
    fn voice(seconds: i64, sound: &[f32]) -> Voice {
        unsteady(seconds, sound, 0.9)
    }
    fn turn(start_ms: i64, end_ms: i64, speaker: i32) -> Turn {
        Turn { start_ms, end_ms, speaker }
    }

    #[test]
    fn one_person_split_into_several_voices_is_joined() {
        let voices = [
            voice(60, &[1.0, 0.0, 0.0, 0.0]),
            voice(50, &[0.0, 0.0, 1.0, 0.0]),
            voice(40, &[0.97, 0.24, 0.0, 0.0]),
            voice(30, &[0.96, 0.0, 0.0, 0.28]),
        ];
        assert_eq!(join_voices(&voices, 0), vec![0, 1, 0, 0]);
    }

    #[test]
    fn different_people_stay_apart() {
        let voices = [
            voice(60, &[1.0, 0.0, 0.0, 0.0]),
            voice(60, &[0.0, 1.0, 0.0, 0.0]),
            voice(60, &[0.0, 0.0, 1.0, 0.0]),
            // Like the first, but less like it than each sounds like itself.
            voice(60, &[0.85, 0.0, 0.0, 0.53]),
        ];
        assert_eq!(join_voices(&voices, 0), vec![0, 1, 2, 3]);
    }

    #[test]
    fn an_unsteady_voice_is_judged_by_how_much_it_varies() {
        // Someone on a poor line: their own pieces are only 0.6 alike, so two
        // halves of them 0.57 alike are still one person.
        let poor_line = [unsteady(90, &[1.0, 0.0, 0.0], 0.6), unsteady(90, &[0.95, 0.31, 0.0], 0.6), voice(90, &[0.0, 0.0, 1.0])];
        assert_eq!(join_voices(&poor_line, 0), vec![0, 0, 2]);
        // Two clear voices that alike are two people.
        let clear = [voice(90, &[1.0, 0.0, 0.0]), voice(90, &[0.67, 0.74, 0.0])];
        assert_eq!(join_voices(&clear, 0), vec![0, 1]);
    }

    #[test]
    fn a_minor_voice_joins_the_voice_it_sounds_most_like() {
        let voices = [
            voice(200, &[1.0, 0.0, 0.0]),
            voice(200, &[0.0, 1.0, 0.0]),
            // Five seconds of 405: too little to be someone new at this likeness.
            voice(5, &[0.5, 0.0, 0.87]),
        ];
        assert_eq!(join_voices(&voices, 0), vec![0, 1, 0]);
    }

    #[test]
    fn a_fragment_joins_someone_even_when_unalike() {
        let voices = [voice(100, &[1.0, 0.0, 0.0]), voice(100, &[0.0, 1.0, 0.0]), voice(2, &[0.1, 0.2, 0.97])];
        assert_eq!(join_voices(&voices, 0), vec![0, 1, 1]);
    }

    #[test]
    fn never_more_voices_than_people_invited() {
        let voices = [
            voice(60, &[1.0, 0.0, 0.0, 0.0]),
            voice(60, &[0.0, 1.0, 0.0, 0.0]),
            voice(60, &[0.0, 0.0, 1.0, 0.0]),
            voice(60, &[0.0, 0.3, 0.0, 0.95]),
        ];
        assert_eq!(join_voices(&voices, 3), vec![0, 1, 2, 1]);
        assert_eq!(join_voices(&voices, 0), vec![0, 1, 2, 3]);
    }

    #[test]
    fn voices_without_pieces_are_left_alone() {
        let voices = [voice(60, &[1.0, 0.0]), Voice { speech_ms: 800, pieces: Vec::new() }, voice(60, &[0.0, 1.0])];
        assert_eq!(join_voices(&voices, 1), vec![0, 1, 0]);
    }

    #[test]
    fn unheard_turns_take_the_nearest_voice() {
        let mut turns = vec![turn(0, 5_000, 0), turn(5_200, 5_800, 7), turn(9_000, 12_000, 1), turn(12_500, 12_900, 8)];
        fold_unheard(&mut turns, &HashSet::from([0, 1]));
        let speakers: Vec<i32> = turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, vec![0, 0, 1, 1]);
    }

    #[test]
    fn joined_turns_that_touch_become_one() {
        let turns = vec![turn(0, 4_000, 0), turn(4_300, 6_000, 0), turn(6_100, 8_000, 1), turn(8_200, 9_000, 0)];
        let joined = join_touching(turns);
        let spans: Vec<(i64, i64, i32)> = joined.iter().map(|t| (t.start_ms, t.end_ms, t.speaker)).collect();
        assert_eq!(spans, vec![(0, 6_000, 0), (6_100, 8_000, 1), (8_200, 9_000, 0)]);
    }

    /// Who really spoke most of each voice's turns, from `<fixture>.truth.csv`.
    fn truly(turns: &[Turn], truth: &[Turn], label: i32) -> (i32, f64) {
        let mut heard: std::collections::HashMap<i32, i64> = Default::default();
        for t in turns.iter().filter(|t| t.speaker == label) {
            for r in truth {
                let overlap = t.end_ms.min(r.end_ms) - t.start_ms.max(r.start_ms);
                if overlap > 0 {
                    *heard.entry(r.speaker).or_default() += overlap;
                }
            }
        }
        let total = heard.values().sum::<i64>().max(1);
        heard.into_iter().max_by_key(|(_, ms)| *ms).map(|(s, ms)| (s, ms as f64 / total as f64)).unwrap_or((-1, 0.0))
    }

    /// TIDY_DIARIZATION_DIR (segmentation.onnx and embedding.onnx),
    /// TIDY_DIARIZATION_FIXTURE (a 16 kHz WAV with a .truth.csv beside it) and
    /// TIDY_DIARIZATION_SPEAKERS (how many people really speak).
    #[test]
    #[ignore = "needs the speaker models and a test call"]
    fn real_call_finds_each_person_once() {
        let dir = PathBuf::from(std::env::var("TIDY_DIARIZATION_DIR").expect("TIDY_DIARIZATION_DIR"));
        let fixture = PathBuf::from(std::env::var("TIDY_DIARIZATION_FIXTURE").expect("TIDY_DIARIZATION_FIXTURE"));
        let people: usize = std::env::var("TIDY_DIARIZATION_SPEAKERS").expect("TIDY_DIARIZATION_SPEAKERS").parse().unwrap();
        let (seg, emb) = (dir.join("segmentation.onnx"), dir.join("embedding.onnx"));
        let truth: Vec<Turn> = std::fs::read_to_string(fixture.with_extension("truth.csv"))
            .unwrap()
            .lines()
            .map(|l| {
                let v: Vec<i64> = l.split(',').map(|x| x.parse().unwrap()).collect();
                turn(v[0], v[1], v[2] as i32)
            })
            .collect();
        let pcm = load_pcm_16k_mono(&fixture).unwrap();

        let started = std::time::Instant::now();
        let found: Vec<Turn> = diarize_with(&seg, &emb, &pcm, -1)
            .unwrap()
            .into_iter()
            .map(|s| turn(s.start_ms, s.end_ms, s.speaker))
            .collect();
        let mut labels: Vec<i32> = found.iter().map(|t| t.speaker).collect::<HashSet<_>>().into_iter().collect();
        labels.sort();
        eprintln!("first pass: {} voices in {:?}", labels.len(), started.elapsed());

        // How alike the voices sound, by who they really are.
        let extractor = sherpa_onnx::SpeakerEmbeddingExtractor::create(&sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: Some(emb.to_string_lossy().to_string()),
            ..Default::default()
        })
        .unwrap();
        // Label, speech, who really spoke it (and how much), and its pieces.
        type Heard = (i32, i64, (i32, f64), Vec<Vec<f32>>);
        let voices: Vec<Heard> = labels
            .iter()
            .map(|l| {
                let theirs: Vec<&Turn> = found.iter().filter(|t| t.speaker == *l).collect();
                let speech = theirs.iter().map(|t| t.end_ms - t.start_ms).sum();
                (*l, speech, truly(&found, &truth, *l), pieces_of(&extractor, &pcm, &theirs))
            })
            .collect();
        let (mut same, mut different) = (Vec::new(), Vec::new());
        for (i, a) in voices.iter().enumerate() {
            eprintln!("voice {:>2}: {:>6.1} s, really {} ({:.0}%), {} pieces, steadiness {:?}", a.0, a.1 as f64 / 1000.0, a.2 .0, a.2 .1 * 100.0, a.3.len(), steadiness(&a.3));
            for b in &voices[i + 1..] {
                if !a.3.is_empty() && !b.3.is_empty() {
                    let close = alike(&a.3, &b.3);
                    if a.2 .0 == b.2 .0 { same.push(close) } else { different.push(close) }
                }
            }
        }
        if let Ok(dump) = std::env::var("TIDY_DIARIZATION_DUMP") {
            let rows: Vec<serde_json::Value> = voices
                .iter()
                .map(|v| serde_json::json!({"label": v.0, "speech_ms": v.1, "truly": v.2 .0, "purity": v.2 .1, "pieces": v.3}))
                .collect();
            let turns: Vec<(i64, i64, i32)> = found.iter().map(|t| (t.start_ms, t.end_ms, t.speaker)).collect();
            std::fs::write(dump, serde_json::json!({"voices": rows, "turns": turns}).to_string()).unwrap();
        }
        let range = |v: &[f32]| (v.iter().cloned().fold(f32::MAX, f32::min), v.iter().cloned().fold(f32::MIN, f32::max));
        eprintln!("same person: {:?} of {}, different people: {:?} of {}", range(&same), same.len(), range(&different), different.len());

        for at_most in [0, people] {
            let joined = rejoin(&emb, &pcm, found.clone(), at_most).unwrap();
            let mut kept: Vec<i32> = joined.iter().map(|t| t.speaker).collect::<HashSet<_>>().into_iter().collect();
            kept.sort();
            let who: Vec<(i32, f64)> = kept.iter().map(|l| truly(&joined, &truth, *l)).collect();
            eprintln!("joined (at most {at_most}): {} voices, really {who:?}", kept.len());
            assert_eq!(kept.len(), people, "{who:?}");
            let distinct: HashSet<i32> = who.iter().map(|(s, _)| *s).collect();
            assert_eq!(distinct.len(), people, "each person is one voice: {who:?}");
        }
    }
}
