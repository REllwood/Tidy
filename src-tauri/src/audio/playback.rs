//! Playing a meeting back. Recordings are stereo, with the microphone on one
//! side and the call on the other, which sounds wrong in headphones, so
//! `tidy-audio://localhost/<page id>` serves them as mono, mixed on the fly
//! for whichever byte range the player asks for. Only recordings registered
//! in the meeting library, inside the recordings folder, can be played.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

use super::pipeline::soft_limit;

pub const SCHEME: &str = "tidy-audio";
/// The most a single range response carries, so seeking stays cheap.
const MAX_CHUNK: u64 = 2 * 1024 * 1024;
const HEADER: u64 = 44;

/// Where a WAV file's samples are and how they are laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    channels: u16,
    rate: u32,
    data_offset: u64,
    frames: u64,
}

fn layout(file: &mut File) -> Option<Layout> {
    let length = file.metadata().ok()?.len();
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff).ok()?;
    if &riff[..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }
    let (mut channels, mut rate, mut bits) = (0u16, 0u32, 0u16);
    let mut offset = 12u64;
    while offset + 8 <= length {
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk).ok()?;
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
        if &chunk[..4] == b"fmt " && size >= 16 {
            let mut fmt = [0u8; 16];
            file.read_exact(&mut fmt).ok()?;
            channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            bits = u16::from_le_bytes([fmt[14], fmt[15]]);
        }
        if &chunk[..4] == b"data" {
            if bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
            let available = (length - (offset + 8)).min(size);
            return Some(Layout {
                channels,
                rate,
                data_offset: offset + 8,
                frames: available / (channels as u64 * 2),
            });
        }
        offset += 8 + size + (size & 1);
    }
    None
}

/// The header of the mono file being served.
fn mono_header(rate: u32, frames: u64) -> [u8; HEADER as usize] {
    let data = (frames * 2) as u32;
    let mut h = [0u8; HEADER as usize];
    h[..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// Bytes `start..=end` of the mono version of the recording.
fn mono_bytes(file: &mut File, layout: Layout, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity((end - start + 1) as usize);
    if start < HEADER {
        let header = mono_header(layout.rate, layout.frames);
        out.extend_from_slice(&header[start as usize..=(end.min(HEADER - 1)) as usize]);
    }
    if end >= HEADER {
        let from = start.max(HEADER) - HEADER;
        let to = end - HEADER; // inclusive
        let (first, last) = (from / 2, to / 2);
        let channels = layout.channels as u64;
        file.seek(SeekFrom::Start(layout.data_offset + first * channels * 2))?;
        let mut raw = vec![0u8; ((last - first + 1) * channels * 2) as usize];
        let read = file.read(&mut raw)?;
        raw.truncate(read - read % (channels as usize * 2));
        let mut mono = Vec::with_capacity(raw.len() / channels as usize);
        for frame in raw.chunks_exact(channels as usize * 2) {
            // Sum the sides exactly; only peaks near full scale are limited.
            let sum: i32 = frame
                .chunks_exact(2)
                .map(|s| i16::from_le_bytes([s[0], s[1]]) as i32)
                .sum();
            let v = if sum.abs() <= 26_214 {
                sum as i16
            } else {
                (soft_limit(sum as f32 / 32768.0).clamp(-1.0, 1.0) * 32767.0) as i16
            };
            mono.extend_from_slice(&v.to_le_bytes());
        }
        let skip = (from % 2) as usize;
        let want = (to - from + 1) as usize;
        out.extend(mono.into_iter().skip(skip).take(want));
    }
    Ok(out)
}

/// "bytes=START-END" or "bytes=START-", clamped to the file.
fn range(header: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = match end.trim() {
        "" => total.saturating_sub(1),
        e => e.parse().ok()?,
    };
    (start < total).then(|| (start, end.min(total - 1).min(start + MAX_CHUNK - 1)))
}

fn audio_path(app: &AppHandle, page_id: &str) -> Option<std::path::PathBuf> {
    let source = {
        let db = app.state::<crate::db::Db>();
        let conn = db.lock().ok()?;
        tidy_core::store::recordings::source(&conn, page_id).ok()?
    };
    let root = super::recorder::app_recordings_dir(app).ok()?.canonicalize().ok()?;
    let path = Path::new(&source).canonicalize().ok()?;
    (path.starts_with(&root) && path.is_file()).then_some(path)
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(status).body(Vec::new()).expect("a valid response")
}

/// Answer one request from the audio player.
pub fn serve(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let page_id = request.uri().path().trim_start_matches('/');
    if page_id.is_empty() || !page_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return empty(StatusCode::BAD_REQUEST);
    }
    let Some(path) = audio_path(app, page_id) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let Ok(mut file) = File::open(&path) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let Some(layout) = layout(&mut file) else {
        return empty(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    };
    let total = HEADER + layout.frames * 2;
    let asked = request.headers().get("range").and_then(|v| v.to_str().ok());
    let (status, start, end) = match range(asked, total) {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
        None if asked.is_some() => return empty(StatusCode::RANGE_NOT_SATISFIABLE),
        None => (StatusCode::OK, 0, total - 1),
    };
    let Ok(body) = mono_bytes(&mut file, layout, start, end) else {
        return empty(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let mut response = Response::builder()
        .status(status)
        .header("Content-Type", "audio/wav")
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", body.len().to_string())
        .header("Access-Control-Allow-Origin", "*");
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header("Content-Range", format!("bytes {start}-{}/{total}", start + body.len() as u64 - 1));
    }
    response.body(body).unwrap_or_else(|_| empty(StatusCode::INTERNAL_SERVER_ERROR))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, frames: &[(i16, i16)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tidy-playback-{}", crate::db::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        let spec = hound::WavSpec { channels, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for (l, r) in frames {
            w.write_sample(*l).unwrap();
            if channels == 2 {
                w.write_sample(*r).unwrap();
            }
        }
        w.finalize().unwrap();
        path
    }

    #[test]
    fn a_stereo_recording_plays_as_a_mono_mix() {
        let path = wav(2, &[(1000, 0), (0, 2000), (500, 500), (-300, 0)]);
        let mut file = File::open(&path).unwrap();
        let layout = layout(&mut file).unwrap();
        assert_eq!(layout.frames, 4);
        let whole = mono_bytes(&mut file, layout, 0, HEADER + 7).unwrap();
        assert_eq!(&whole[..4], b"RIFF");
        let reader = hound::WavReader::new(std::io::Cursor::new(whole)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
        // Each side keeps its full level: the mic alone, the call alone, both.
        assert_eq!(samples.len(), 4);
        assert_eq!(samples, vec![1000, 2000, 1000, -300]);
    }

    #[test]
    fn any_byte_range_matches_the_same_bytes_of_the_whole_file() {
        let frames: Vec<(i16, i16)> = (0..50).map(|i| (i * 100, -i * 30)).collect();
        let path = wav(2, &frames);
        let mut file = File::open(&path).unwrap();
        let layout = layout(&mut file).unwrap();
        let total = HEADER + layout.frames * 2;
        let whole = mono_bytes(&mut file, layout, 0, total - 1).unwrap();
        for (start, end) in [(0, 10), (40, 50), (45, 46), (47, 90), (100, total - 1)] {
            assert_eq!(mono_bytes(&mut file, layout, start, end).unwrap(), whole[start as usize..=end as usize], "{start}-{end}");
        }
    }

    #[test]
    fn older_mono_recordings_play_unchanged() {
        let path = wav(1, &[(1200, 0), (-800, 0)]);
        let mut file = File::open(&path).unwrap();
        let layout = layout(&mut file).unwrap();
        let bytes = mono_bytes(&mut file, layout, HEADER, HEADER + 3).unwrap();
        assert_eq!(bytes, [1200i16.to_le_bytes(), (-800i16).to_le_bytes()].concat());
    }

    #[test]
    fn ranges_are_read_and_kept_small() {
        assert_eq!(range(Some("bytes=0-1"), 100), Some((0, 1)));
        assert_eq!(range(Some("bytes=10-"), 100), Some((10, 99)));
        assert_eq!(range(Some("bytes=10-500"), 100), Some((10, 99)));
        assert_eq!(range(Some("bytes=0-"), 10 * MAX_CHUNK), Some((0, MAX_CHUNK - 1)));
        assert_eq!(range(Some("bytes=100-"), 100), None);
        assert_eq!(range(None, 100), None);
    }
}
