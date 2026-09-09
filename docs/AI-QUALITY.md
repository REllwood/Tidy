# Meeting AI quality checks

The regression fixture reconstructs a client-handover conversation from a reported failure: an offhand “fly-by-night” remark was incorrectly promoted into the meeting's subject. It is synthetic, not a copy of a user's recording.

The executable evaluation uses the pinned Qwen3 4B answer model, Nomic embeddings, the SQLite retrieval code and the same note/answer functions as the desktop app. It checks the handover topic, named responsibility, a competing client's exclusion, an unanswered salary question, a rejected Friday proposal and an unconfirmed billing contact. It also checks that a promise to book something is not presented as an existing booking.

Run it after building the AI runtime and downloading the pinned model fixtures:

```sh
DYLD_FALLBACK_LIBRARY_PATH=/usr/lib/swift \
TIDY_AI_MODEL_DIR=/absolute/path/to/model-fixtures \
cargo test -p tidy --release --lib real_handover_quality_regressions --locked -- --ignored --nocapture
```

The fixture directory contains `qwen3-4b.gguf` and `nomic-embed.gguf`. Tests use an in-memory database and do not read the user's meeting library.

Unit tests separately cover contextual transcript windows, source invalidation, old-index rebuilding, client/date filtering, numeric evidence and safe rendering of generated content. The embedding batch and micro-batch sizes are both 2,048, allowing the bounded conversation windows to be processed together. This follows the [embedding batch requirement in llama.cpp](https://github.com/ggml-org/llama.cpp/blob/427291b5b34cd914a31b3fd3b61a68f6184f4b9f/tools/server/server.cpp#L143).

These are regression checks, not an accuracy benchmark across real customer meetings. Local models can still miss nuance, omit information or make incorrect inferences. A short isolated discussion remark is excluded from an overview when its vocabulary has no support elsewhere in the conversation; that conservative rule can omit a brief, unique topic. Large libraries are sampled within the context budget and show their coverage. Original transcripts and source links remain available for review. Speaker identification and transcription quality also affect the result.

Verified on 9 September 2026 using the balanced local model: the executable regression passed the overview, handover owner, client isolation, unknown salary, rejected proposal and pending billing-contact checks. The reconstructed overview identified client handovers and pack requirements without the passing remark or its “rushed” paraphrase. These results do not cover the original recording, which was not supplied.
