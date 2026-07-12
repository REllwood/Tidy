//! On-device speaker diarization via sherpa-onnx (pyannote segmentation + a
//! speaker-embedding model + fast clustering). Models live in
//! `app_data/models/diarization/{segmentation.onnx, embedding.onnx}`.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::whisper::transcribe::load_pcm_16k_mono;

#[derive(Debug, Serialize, Clone)]
pub struct SpeakerSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: i32,
}

pub fn diarization_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(format!("app_data_dir: {e}")))?
        .join("models")
        .join("diarization");
    Ok(dir)
}

fn model_paths(app: &AppHandle) -> AppResult<(PathBuf, PathBuf)> {
    let dir = diarization_dir(app)?;
    let seg = dir.join("segmentation.onnx");
    let emb = dir.join("embedding.onnx");
    if !seg.exists() || !emb.exists() {
        return Err(AppError::Invalid(
            "diarization models not installed".into(),
        ));
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
    use sherpa_onnx::{
        FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    };

    let (seg, emb) = model_paths(app)?;
    let pcm = load_pcm_16k_mono(wav)?;
    if pcm.is_empty() {
        return Ok(vec![]);
    }

    let mut config = OfflineSpeakerDiarizationConfig::default();
    config.segmentation.pyannote.model = Some(seg.to_string_lossy().to_string());
    config.embedding.model = Some(emb.to_string_lossy().to_string());
    config.clustering = FastClusteringConfig {
        num_clusters: if num_speakers > 0 { num_speakers } else { -1 },
        threshold: 0.5,
    };

    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| AppError::Other("failed to init diarizer".into()))?;
    let result = diarizer
        .process(&pcm)
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
    tauri::async_runtime::spawn_blocking(move || {
        diarize_file(&app2, &canon_audio, num_speakers)
    })
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
        .user_agent("AppFlower/0.1")
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
