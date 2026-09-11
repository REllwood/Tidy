# Local AI and meeting questions

Tidy can run summaries, writing assistance and meeting questions using a bundled
llama.cpp runtime. Ollama remains an optional advanced provider. AI is disabled
until the user enables it or selects a downloaded answer model.

Recording-history and transcription changes in v0.3.0 are described in
[Recording reliability](MEETING-RELIABILITY.md), including retention and recovery limits.

## User flow

In Settings > Local AI, Enable local AI downloads the recommended answer model
and the meeting search model. Downloads show progress and can be cancelled.
The model catalogue shows disk use, model selection and explicit removal controls.
Removal requires confirmation and never removes meetings or transcripts.
Interrupted downloads are retained as partial files; Remove files also clears
those partials after confirmation. Retry starts a fresh download.

Version 0.3.0 includes the expanded model catalogue. Setup suggests Qwen 0.6B below 8 GB,
1.7B at 8 GB, 4B at 12–16 GB, 8B at 24 GB, 14B at 32–48 GB and 32B at 64 GB
or more. If RAM cannot be detected, it defaults to 4B. These are starting points,
not benchmark results or minimum requirements. Manual selection remains available
for every model, with a notice when the Mac has less RAM than suggested.
Models are loaded one at a time with an 8,192-token context and
one inference slot. Starting a recording cancels current AI work and pauses the
queue until the recording/transcription flow finishes. The inference process is
stopped after each operation, on cancellation/error, and on application exit.

Recordings save their original transcript before optional speaker labelling or AI
processing. Speaker labelling uses a conditional write so it cannot replace edits
made after the transcript was saved. Recording state survives navigation within
the app, with a visible return-to-recorder control.

Summaries and search indexing run from a persisted queue while Tidy is open.
Interrupted jobs resume after restart; failed/cancelled jobs show their error and
can be retried in Ask meetings. Existing recorded meetings enter the queue on
upgrade, but do not process until AI is enabled. Generated summaries are displayed
separately above the original document and never overwrite the transcript.
Notes are organised into topic sections, decisions, next steps and open questions.
They retain verified points rather than repeatedly compressing summary paragraphs.
The Regenerate notes control rebuilds the notes and search index for a meeting.
Older index versions are queued for rebuilding on startup; original transcripts
are retained. The additional evidence checks take extra inference time.

Generated action items are suggestions; this feature does not automatically create
planner tasks. These generated summaries are not currently included in Markdown
vault exports, which continue to export the original page content.

## Retrieval and answer limits

Only content below a Transcript heading in a recorded meeting is indexed. Summary
text is excluded. Transcript passages retain their source page, block and timestamp.
The local index combines SQLite FTS5 keyword ranking and Nomic cosine similarity
using reciprocal-rank fusion. Short turns and speaker labels are grouped into
conversational windows, with overlap at boundaries. Specific questions retrieve up
to ten passages within a 12,000-character evidence budget. Generic question words
are excluded from keyword ranking. Broad overview questions instead select material
across the beginning, middle and end of the scoped meetings. The answer shows how
many indexed passages and meetings were read; a sampled overview is not a complete
review of a large library.
Client and date filters restrict meetings before ranking; the Through date is inclusive.
Queries spanning more than 50,000 passages require a narrower date range.

Edits invalidate the derived summary and index through SQLite triggers. Deleted
meetings are excluded. Results are checked again after generation so sources edited
or deleted while answering are not returned as current evidence. Embedding vectors
are tied to a fixed model version; changing that model requires a re-index.

Answers are assembled as individual claims with inline source links. Each generated
claim must include a verbatim quote from its cited passage. Unsupported quotes,
numeric claims and common completed-versus-planned status changes are filtered out.
A separate model pass checks entailment, attribution, negation and certainty. When the model provides
no supporting sources, Tidy shows an insufficient-evidence response. These checks
reduce certain failure modes; they do not prove semantic correctness. Names,
commitments, indirect references and transcription errors still need human review.
Questions and their last answer remain available while opening source notes and
returning to the library, but are not saved after quitting. Each question is independent.

## Client history and progress

Choose an existing client suggestion or enter a client name when recording. Repeated
names reuse the same client page (trimmed and case-insensitive for ASCII names).
The saved client relationship connects each meeting to that client's library, even
if its page is subsequently moved. Renaming the client updates its displayed name.
Meetings without a client remain available under All clients. This version does not
provide a dedicated client reassignment or duplicate-client merge control.

Select a client in Ask meetings to search across their recordings. Sources are given
to the model in meeting-date order so original and revised decisions can be compared.
Other clients' passages are excluded before retrieval; a deleted client cannot be
queried by its old identifier. All clients deliberately searches the whole library.

The readiness percentage counts fully indexed meetings in the selected client/date
scope; it is not an estimate of elapsed processing time. Queued, active and failed
meeting counts are shown separately. Active passage progress describes the current
stage only. Summaries and indexing have separate workloads, so their percentages
must not be interpreted as a whole-meeting time estimate.

## Models and runtime

| Answer model | Download | Suggested total RAM |
| --- | --- | --- |
| Qwen 3 0.6B Q8_0 | 639 MB | 4 GB or more |
| Qwen 3 1.7B Q8_0 | 1.8 GB | 8 GB or more |
| Qwen 3 4B Q4_K_M | 2.5 GB | 12 GB or more |
| Qwen 3 8B Q4_K_M | 5.0 GB | 16 GB or more |
| Qwen 3 14B Q4_K_M | 9.0 GB | 24 GB or more |
| Qwen 3 32B Q4_K_M | 19.8 GB | 48 GB or more |

Meeting search uses Nomic Embed Text v1.5 Q8_0 (146 MB) alongside the selected
answer model. Larger models give users more options; they do not guarantee more
accurate meeting answers. Download size describes disk space. RAM guidance includes
headroom for macOS and other apps and has not been benchmarked across these machines.

Transcription has 12 Whisper choices: Tiny, Base and Small in multilingual and
English-only versions; Medium in full and compressed versions; and Large V3 and
Large V3 Turbo, each in full and compressed versions. Downloads range from 78 MB
to 3.1 GB. Compressed versions use reduced precision and may change accuracy.
Whisper and the answer model are chosen separately.

The native app and preview share `src/lib/modelCatalogue.json`. It records exact
file sizes, fixed upstream revisions and SHA-256 hashes. Answer model downloads
and loads are verified; transcription downloads are verified before they become
selectable. Existing model IDs and file locations are preserved.

Run `node scripts/verify-model-catalogue.mjs` to verify the pinned revision, file
size and checksum against Hugging Face headers without downloading model weights.
A complete hardware and transcription-quality comparison remains to be done.

The runtime is built from llama.cpp commit
`427291b5b34cd914a31b3fd3b61a68f6184f4b9f`. It binds only to loopback, uses a fresh
private API key, disables its web UI and does not log prompts. Local HTTP clients
ignore proxy settings and redirects. Ollama selections are checked for local chat
capability before sending text; remote/cloud models are rejected.

The signed app bundles the runtime and its MIT licence. Model references and licences:
[Qwen3 4B](https://huggingface.co/Qwen/Qwen3-4B-GGUF),
[Qwen3 0.6B](https://huggingface.co/Qwen/Qwen3-0.6B-GGUF),
[Nomic Embed Text](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5-GGUF).

The separately invoked MCP sidecar retains its existing Ollama-based summarisation
path. It does not start the app's managed runtime. Desktop File a note, writing
assistance and meeting summaries use the selected managed provider.

## Development and verification

Build the runtime before a native development session:

```bash
bash scripts/build-ai-runtime.sh
npm run tauri dev
```

`npm run release:build` also builds and stages the pinned runtime, signs it as a
nested executable, and includes it in release verification. It does not publish.

The browser preview clearly labels its simulated setup and answers. It is useful
for UI checks but does not demonstrate native inference.

The real runtime smoke test uses synthetic meeting passages and never opens the
user's database. Provide already-downloaded, pinned model files:

```bash
python3 scripts/smoke-ai-runtime.py /path/to/tidy-ai /path/to/qwen3-4b.gguf /path/to/nomic-embed.gguf
```

The optional native integration test uses the same files under a fixture directory:

```bash
TIDY_AI_MODEL_DIR=/path/to/fixtures DYLD_FALLBACK_LIBRARY_PATH=/usr/lib/swift cargo test -p tidy --lib real_runtime_roundtrip_and_cancellation -- --ignored
```

Initial synthetic tests on this development Mac (32 GiB RAM) measured sampled
process RSS of approximately 3.9 GB for the answer model and 0.24 GB for the search
model, running separately. Short answers took approximately 0.5–1.5 seconds after
loading. These are sample results, not a full memory profile or M1/16 GB benchmark.
Long meetings, battery impact and memory pressure alongside other apps require
additional hardware testing before promising those results to users.
