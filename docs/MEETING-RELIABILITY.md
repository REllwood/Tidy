# Recordings and transcript history

Available from v0.3.0.

## Audio is optional

**Keep meeting audio** is off by default in Settings. Transcripts are saved either
way. With the setting off, Tidy uses a temporary recording for transcription and
speaker labels, then discards it—even if transcription fails. Temporary files left
by an interruption are cleared when Tidy next opens.

Turn the setting on before recording if you want to listen to the audio later or
try transcription again. The choice is fixed when recording starts. Turning it
off later does not remove existing saved recordings. Saved audio uses roughly
115 MB per hour.

Without a transcription model, you must enable audio retention before recording.
This lets you download a model and transcribe the meeting later.

## Meeting history

Recording history lists meetings by client and date. Open a meeting to review the
transcript, export text or choose an earlier text version. If its audio was kept,
you can export the audio or transcribe it again with another downloaded Whisper
model and a spoken-language setting. Processing shows progress and can be cancelled.

A successful retry archives the previous text. If someone edits the page while
transcription is running, the candidate is saved as a separate version and the
edited page is kept. Updated transcripts queue their notes and search index for
rebuilding when local AI is enabled.

Re-transcription needs audio. Saved text can be corrected or used to regenerate
notes, but cannot recover words that were missed. Older versions deleted audio
after transcription; this update cannot restore those files.

## Current limits

Audio is buffered until Stop. A crash during recording can lose that unfinished
capture. Incremental recovery, echo cancellation and a second transcription engine
have not been added.

The final Whisper pass uses beam search, bounded thread counts and an explicit
language choice. Mixing avoids hard clipping. These changes and larger model
choices need evaluation against real meeting audio; they are not an accuracy
guarantee or a 16 GB MacBook benchmark.

## Meetily references

We reviewed Meetily at
[`a2cb62e827da7ef59f65064c97233efb2313878e`](https://github.com/Zackriya-Solutions/meetily/tree/a2cb62e827da7ef59f65064c97233efb2313878e),
particularly its retained recordings, model/language controls and
[re-transcription flow](https://github.com/Zackriya-Solutions/meetily/blob/a2cb62e827da7ef59f65064c97233efb2313878e/frontend/src-tauri/src/audio/retranscription.rs).
Tidy's implementation uses its existing recorder, database and editor. No Meetily
source was copied. Its [MIT licence](https://github.com/Zackriya-Solutions/meetily/blob/a2cb62e827da7ef59f65064c97233efb2313878e/LICENSE.md)
requires preserving the licence notice if source is reused later.

## Verification

Tests cover database upgrades, retention defaults, transcript versions, concurrent
edits, text export and source-file preservation. A separate real Whisper test uses
synthetic speech containing “client” and “handover”, transcribes it twice, cancels
another pass, and verifies that the source bytes stay unchanged:

```bash
TIDY_WHISPER_MODEL=/path/to/ggml-tiny.en.bin \
TIDY_TRANSCRIPTION_FIXTURE=/path/to/handover.wav \
DYLD_FALLBACK_LIBRARY_PATH=/usr/lib/swift \
cargo test -p tidy --lib real_saved_audio_can_be_transcribed_twice -- --ignored
```

The fixture uses a separate directory and does not open a user database. Native
recording permissions, save dialogues and long meetings still need app-level tests.
