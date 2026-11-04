//! Turning Whisper's raw segments into a meeting transcript: deciding which
//! side of the call to transcribe, dropping lines Whisper invents in silence,
//! removing the other side's voice where it echoes into the microphone, and
//! labelling who spoke.

use serde::Serialize;

use super::transcribe::TranscriptSegment;
use crate::audio::pipeline::WHISPER_RATE;

/// Where a run of audio came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The microphone, while the other side of the call was captured separately.
    Mic,
    /// The other side of a call, captured from the Mac's audio output.
    System,
    /// One track holding everyone: an older mono recording, or a meeting in a
    /// room where only the microphone heard anything.
    Mixed,
}

/// A transcript line with the speaker it belongs to, when that is known.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Labelled {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// The label for the person recording, on the microphone side of a call.
pub const YOU: &str = "You";
/// The label for the other side of a call when speakers are not told apart.
pub const OTHERS: &str = "Others";

const FRAME: usize = (WHISPER_RATE / 1000 * 30) as usize; // 30 ms
/// A frame this loud counts as someone speaking (about -40 dBFS).
const VOICED_RMS: f32 = 0.01;
/// A channel with less speech than this is treated as silent.
const MIN_SPEECH_SECONDS: f32 = 2.0;

fn frame_rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

/// Seconds of a 16 kHz channel that sound like someone speaking.
pub fn speech_seconds(pcm: &[f32]) -> f32 {
    let voiced = pcm
        .chunks(FRAME)
        .filter(|f| frame_rms(f) >= VOICED_RMS)
        .count();
    voiced as f32 * FRAME as f32 / WHISPER_RATE as f32
}

pub fn has_speech(pcm: &[f32]) -> bool {
    speech_seconds(pcm) >= MIN_SPEECH_SECONDS
}

/// True when no part of the span sounds like speech. Whisper's timestamps are
/// loose, so the span is widened a little before deciding.
fn silent_span(pcm: &[f32], start_ms: i64, end_ms: i64) -> bool {
    let rate = WHISPER_RATE as i64;
    let from = ((start_ms - 300).max(0) * rate / 1000) as usize;
    let to = (((end_ms + 300).max(0) * rate / 1000) as usize).min(pcm.len());
    if from >= to {
        return true;
    }
    !pcm[from..to].chunks(FRAME).any(|f| frame_rms(f) >= VOICED_RMS)
}

fn normalise(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn words(text: &str) -> Vec<String> {
    normalise(text).split(' ').filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

/// Lines Whisper is known to produce from silence or music, copied from the
/// subtitles it was trained on.
const INVENTED: &[&str] = &[
    "thank you for watching",
    "thanks for watching",
    "thank you so much for watching",
    "please subscribe",
    "like and subscribe",
    "subscribe to my channel",
    "don't forget to subscribe",
    "subtitles by",
    "subtitles created by",
    "captions by",
    "transcribed by",
    "translated by",
    "amara org",
    "see you in the next video",
];

/// A segment that is only a sound description, such as "[Music]" or
/// "(upbeat music)", or one of Whisper's subtitle credits.
fn invented(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let bracketed = (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
        || trimmed.chars().all(|c| c == '♪' || c.is_whitespace() || c.is_ascii_punctuation());
    if bracketed {
        return true;
    }
    let plain = normalise(trimmed);
    INVENTED.iter().any(|line| plain == *line || plain.starts_with(line))
}

/// Whisper sometimes loops, writing the same few words over and over.
fn repetitive(text: &str) -> bool {
    let w = words(text);
    if w.len() < 8 {
        return false;
    }
    let unique: std::collections::HashSet<&String> = w.iter().collect();
    if (unique.len() as f32) < w.len() as f32 * 0.3 {
        return true;
    }
    let mut run = 1;
    for pair in w.windows(2) {
        run = if pair[0] == pair[1] { run + 1 } else { 1 };
        if run > 6 {
            return true;
        }
    }
    false
}

/// Drop the lines Whisper made up: sound descriptions, subtitle credits,
/// loops, and anything placed where the channel was silent.
pub fn keep_spoken(segments: Vec<TranscriptSegment>, pcm: &[f32]) -> Vec<TranscriptSegment> {
    segments
        .into_iter()
        .filter(|s| !invented(&s.text) && !repetitive(&s.text))
        .filter(|s| !silent_span(pcm, s.start_ms, s.end_ms))
        .collect()
}

/// Whisper often starts a line at the beginning of its window when the words
/// come later, for example after the other side has been talking. Move each
/// line's start and end in to where its channel actually has speech, so the
/// two sides merge in the right order.
pub fn fit_to_speech(segments: Vec<TranscriptSegment>, pcm: &[f32]) -> Vec<TranscriptSegment> {
    let rate = WHISPER_RATE as i64;
    segments
        .into_iter()
        .map(|mut seg| {
            let from = ((seg.start_ms.max(0) * rate / 1000) as usize).min(pcm.len());
            let to = ((seg.end_ms.max(0) * rate / 1000) as usize).min(pcm.len());
            if from >= to {
                return seg;
            }
            let voiced: Vec<usize> = pcm[from..to]
                .chunks(FRAME)
                .enumerate()
                .filter(|(_, f)| frame_rms(f) >= VOICED_RMS)
                .map(|(i, _)| i)
                .collect();
            if let (Some(first), Some(last)) = (voiced.first(), voiced.last()) {
                let ms = |samples: usize| samples as i64 * 1000 / rate;
                seg.start_ms = ms(from + first * FRAME);
                seg.end_ms = ms((from + (last + 1) * FRAME).min(to)).max(seg.start_ms);
            }
            seg
        })
        .collect()
}

/// Longest common subsequence of two word lists.
fn common_words(a: &[String], b: &[String]) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y { diagonal + 1 } else { above.max(row[j]) };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Remove microphone lines that are the other side's voice coming back
/// through the speakers. A line is an echo when it overlaps the other side in
/// time and nearly all of its words were said there at that moment.
pub fn remove_echo(mic: Vec<TranscriptSegment>, system: &[TranscriptSegment]) -> Vec<TranscriptSegment> {
    const SLACK_MS: i64 = 1_500;
    mic.into_iter()
        .filter(|m| {
            let nearby: Vec<&TranscriptSegment> = system
                .iter()
                .filter(|s| s.end_ms + SLACK_MS > m.start_ms && s.start_ms - SLACK_MS < m.end_ms)
                .collect();
            if nearby.is_empty() {
                return true;
            }
            let overlap: i64 = nearby
                .iter()
                .map(|s| (m.end_ms.min(s.end_ms) - m.start_ms.max(s.start_ms)).max(0))
                .sum();
            let shortest = nearby
                .iter()
                .map(|s| s.end_ms - s.start_ms)
                .chain(std::iter::once(m.end_ms - m.start_ms))
                .min()
                .unwrap_or(0)
                .max(1);
            let mine = words(&m.text);
            let theirs: Vec<String> = nearby.iter().flat_map(|s| words(&s.text)).collect();
            if mine.is_empty() {
                return false;
            }
            let shared = common_words(&mine, &theirs) as f32 / mine.len() as f32;
            // A short "yes" or "right" is easily said by both sides, so short
            // lines must match completely and overlap more before they go.
            let (needed_overlap, needed_share) = if mine.len() < 3 { (0.5, 1.0) } else { (0.35, 0.8) };
            let echo = overlap as f32 >= needed_overlap * shortest as f32 && shared >= needed_share;
            !echo
        })
        .collect()
}

/// Speaker turns from diarisation, numbered by who spoke first.
#[derive(Debug, Clone)]
pub struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: i32,
}

/// Give each segment the diarised speaker it overlaps most, as "Speaker N"
/// numbered in order of first appearance so the labels read naturally.
pub fn assign_speakers(segments: &[TranscriptSegment], turns: &[Turn]) -> Vec<Option<String>> {
    let mut order: Vec<i32> = Vec::new();
    segments
        .iter()
        .map(|seg| {
            let best = turns
                .iter()
                .map(|t| (t.speaker, seg.end_ms.min(t.end_ms) - seg.start_ms.max(t.start_ms)))
                .filter(|(_, overlap)| *overlap > 0)
                .max_by_key(|(_, overlap)| *overlap)
                .map(|(speaker, _)| speaker)?;
            let index = match order.iter().position(|s| *s == best) {
                Some(i) => i,
                None => {
                    order.push(best);
                    order.len() - 1
                }
            };
            Some(format!("Speaker {}", index + 1))
        })
        .collect()
}

/// Label one source's segments and merge everything into one timeline.
pub fn merge(sources: Vec<(Vec<TranscriptSegment>, Vec<Option<String>>)>) -> Vec<Labelled> {
    let mut all: Vec<Labelled> = sources
        .into_iter()
        .flat_map(|(segments, labels)| {
            segments.into_iter().zip(labels).map(|(s, speaker)| Labelled {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text,
                speaker,
            })
        })
        .collect();
    all.sort_by_key(|s| (s.start_ms, s.end_ms));
    all
}

fn timestamp(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60)
}

/// The transcript as BlockNote blocks: a heading, then each speaker's name
/// before their turn and one "[hh:mm:ss] text" paragraph per line.
pub fn transcript_blocks(lines: &[Labelled]) -> Vec<serde_json::Value> {
    let mut blocks = vec![serde_json::json!({"type":"heading","props":{"level":2},"content":"Transcript"})];
    let mut current: Option<&str> = None;
    for line in lines {
        if let Some(speaker) = line.speaker.as_deref() {
            if current != Some(speaker) {
                blocks.push(serde_json::json!({"type":"paragraph","content":speaker}));
                current = Some(speaker);
            }
        }
        blocks.push(serde_json::json!({
            "type": "paragraph",
            "content": format!("[{}] {}", timestamp(line.start_ms), line.text),
        }));
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: i64, end: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment { start_ms: start, end_ms: end, text: text.into() }
    }

    fn tone(seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(seconds * WHISPER_RATE as f32) as usize)
            .map(|i| (i as f32 / 16.0).sin() * amplitude)
            .collect()
    }

    #[test]
    fn quiet_channels_are_not_transcribed() {
        assert!(!has_speech(&vec![0.0; WHISPER_RATE as usize * 60]));
        assert!(!has_speech(&tone(60.0, 0.002)), "room noise is not speech");
        let mut talk = vec![0.0; WHISPER_RATE as usize * 30];
        talk.extend(tone(3.0, 0.1));
        assert!(has_speech(&talk));
    }

    #[test]
    fn whisper_inventions_are_dropped_but_real_lines_stay() {
        let mut pcm = tone(10.0, 0.1);
        pcm.extend(vec![0.0; WHISPER_RATE as usize * 10]);
        let kept = keep_spoken(
            vec![
                seg(0, 3_000, "We moved the launch to the 28th."),
                seg(3_000, 4_000, "[Music]"),
                seg(4_000, 5_000, "(upbeat music)"),
                seg(5_000, 6_000, "Thanks for watching!"),
                seg(6_000, 7_000, "Subtitles by the Amara.org community"),
                seg(7_000, 9_000, "the the the the the the the the the the"),
                seg(14_000, 16_000, "Thank you."),
            ],
            &pcm,
        );
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].text, "We moved the launch to the 28th.");
    }

    #[test]
    fn lines_start_where_their_channel_has_speech() {
        let mut pcm = vec![0.0; WHISPER_RATE as usize * 6];
        pcm.extend(tone(4.0, 0.1));
        // Whisper said the line ran from the start of its window.
        let fitted = fit_to_speech(vec![seg(0, 10_000, "The account owner by Friday.")], &pcm);
        assert!((fitted[0].start_ms - 6_000).abs() <= 30, "{}", fitted[0].start_ms);
        assert!((fitted[0].end_ms - 10_000).abs() <= 30, "{}", fitted[0].end_ms);
        // A line with no speech under it is left as it was.
        let kept = fit_to_speech(vec![seg(0, 2_000, "x")], &pcm);
        assert_eq!(kept[0].start_ms, 0);
    }

    #[test]
    fn echo_of_the_other_side_is_removed_from_the_mic() {
        let system = vec![
            seg(0, 4_000, "Can you send the revised timeline to Dr Chen by Friday?"),
            seg(9_000, 11_000, "Great, thanks."),
        ];
        let mic = vec![
            // Their voice coming back through the speakers, slightly garbled.
            seg(200, 4_100, "you send the revised timeline to Dr Chen by Friday"),
            // The person recording, answering.
            seg(4_500, 8_000, "Yes, I will send it on Thursday."),
            // A short reply just after theirs, sharing a word but not the moment.
            seg(11_200, 11_700, "Great."),
        ];
        let kept = remove_echo(mic, &system);
        let texts: Vec<&str> = kept.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["Yes, I will send it on Thursday.", "Great."]);

        // The same short words at the same moment are their voice, not a reply.
        let kept = remove_echo(vec![seg(9_100, 10_900, "Great, thanks.")], &system);
        assert!(kept.is_empty());
    }

    #[test]
    fn speakers_are_numbered_in_order_of_first_appearance() {
        let segments = vec![seg(0, 1_000, "a"), seg(1_000, 2_000, "b"), seg(2_000, 3_000, "c"), seg(9_000, 9_500, "d")];
        let turns = vec![
            Turn { start_ms: 0, end_ms: 1_100, speaker: 4 },
            Turn { start_ms: 1_100, end_ms: 2_000, speaker: 0 },
            Turn { start_ms: 2_000, end_ms: 3_000, speaker: 4 },
        ];
        assert_eq!(
            assign_speakers(&segments, &turns),
            vec![Some("Speaker 1".into()), Some("Speaker 2".into()), Some("Speaker 1".into()), None]
        );
    }

    #[test]
    fn both_sides_merge_into_one_timeline_with_speaker_headers() {
        let lines = merge(vec![
            (vec![seg(5_000, 6_000, "Sounds good."), seg(0, 2_000, "Morning all.")], vec![Some(YOU.into()), Some(YOU.into())]),
            (vec![seg(2_500, 4_500, "Morning. Shall we start?")], vec![Some("Speaker 1".into())]),
        ]);
        let blocks = transcript_blocks(&lines);
        let content: Vec<&str> = blocks.iter().map(|b| b["content"].as_str().unwrap()).collect();
        assert_eq!(
            content,
            vec![
                "Transcript",
                "You",
                "[00:00:00] Morning all.",
                "Speaker 1",
                "[00:00:02] Morning. Shall we start?",
                "You",
                "[00:00:05] Sounds good.",
            ]
        );
    }

    #[test]
    fn unlabelled_lines_have_no_speaker_headers() {
        let lines = merge(vec![(vec![seg(61_000, 62_000, "Hello.")], vec![None])]);
        let blocks = transcript_blocks(&lines);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1]["content"], "[00:01:01] Hello.");
    }
}
