const savers = new Set<() => Promise<void>>();
const writes = new Set<Promise<unknown>>();

/** How many times we re-run the savers while new writes keep appearing. */
const MAX_DRAIN_ROUNDS = 10;

/**
 * Commands that only read. Everything else is treated as a write and is
 * awaited before the app restarts or closes, so a new command is safe by
 * default rather than silently untracked.
 */
const READ_PREFIXES = ["get_", "list_", "check_"];
const READ_COMMANDS = new Set([
  "search",
  "is_recording",
  "recording_history",
  "transcript_versions",
  "cancel_transcription",
  "diarization_available",
  "diarize",
  "transcribe",
  "local_ai_status",
  "local_ai_cancel",
  "ollama_status",
  "ai_generate",
  "ask_meetings",
  "meeting_ai_jobs",
  "meeting_speakers",
  "meeting_hud_state",
  "live_transcript",
  "live_recap",
  "note_templates",
  "meeting_last_time",
  "meeting_event",
  "calendar_status",
  "calendar_events",
  "note_summary_status",
  "mcp_get_token",
  "confirm_close",
]);

/** True when a command cannot change stored state, so it needs no tracking. */
export function isReadCommand(cmd: string): boolean {
  return READ_PREFIXES.some((p) => cmd.startsWith(p)) || READ_COMMANDS.has(cmd);
}

export function registerPendingEdits(save: () => Promise<void>) {
  savers.add(save);
  return () => {
    savers.delete(save);
  };
}

export function trackWrite<T>(write: Promise<T>): Promise<T> {
  writes.add(write);
  void write.then(
    () => writes.delete(write),
    () => writes.delete(write),
  );
  return write;
}

/**
 * Best-effort flush of everything the editors are holding, used before we read
 * a page back (export). Save failures are surfaced by the editor itself, so
 * they must not stop the caller.
 */
export async function flushPendingEdits(): Promise<void> {
  await Promise.allSettled([...savers].map((save) => save()));
  await Promise.allSettled([...writes]);
}

/**
 * Flush unsaved editor work and wait for the writes it starts, looping until
 * nothing is left in flight (bounded, so a stuck command cannot hang a
 * restart forever). The first failure is re-thrown once the loop finishes.
 */
export async function saveBeforeRestart(): Promise<void> {
  let failure: { reason: unknown } | null = null;
  for (let round = 0; round < MAX_DRAIN_ROUNDS; round += 1) {
    const inFlight = [...writes];
    if (!savers.size && inFlight.length === 0) break;
    const results = await Promise.allSettled([
      ...[...savers].map((save) => save()),
      ...inFlight,
    ]);
    if (!failure) {
      const rejected = results.find((r) => r.status === "rejected");
      if (rejected) failure = { reason: rejected.reason };
    }
    if (writes.size === 0) break;
  }
  if (failure) throw failure.reason;
}
