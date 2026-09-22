import type { SavedRecording } from "@/lib/api";
import type { AiProgress, MeetingJob } from "@/lib/localAi";

/**
 * Polling rules for the meeting surfaces.
 *
 * The recording and summary panels render on every document page, so an idle
 * page must not poll at all: each surface asks for a live interval only while
 * work is actually running, and otherwise relies on query invalidation.
 */
export const ACTIVE_POLL_MS = 2000;

/** States that mean the transcription worker is doing something right now. */
const TRANSCRIBING = "transcribing";
/** States that mean the local AI worker is doing something right now. */
const AI_RUNNING = ["summarising", "indexing"];

export function isMeetingPage(
  recordings: SavedRecording[] | undefined,
  pageId: string,
): boolean {
  return !!recordings?.some((r) => r.page_id === pageId);
}

/** Poll the history only while this page (or this tab's own run) is transcribing. */
export function recordingPanelInterval(
  recordings: SavedRecording[] | undefined,
  pageId: string,
  running: boolean,
): number | false {
  if (running) return ACTIVE_POLL_MS;
  const meeting = recordings?.find((r) => r.page_id === pageId);
  return meeting?.transcript_state === TRANSCRIBING ? ACTIVE_POLL_MS : false;
}

/** Poll the whole history list only while at least one meeting is transcribing. */
export function recordingHistoryInterval(
  recordings: SavedRecording[] | undefined,
  running = false,
): number | false {
  if (running) return ACTIVE_POLL_MS;
  return recordings?.some((r) => r.transcript_state === TRANSCRIBING)
    ? ACTIVE_POLL_MS
    : false;
}

/**
 * A queued job only moves when local AI is ready, so a queued job on a Mac with
 * AI switched off must not keep a timer alive forever.
 */
export function jobIsActive(job: MeetingJob, aiReady: boolean): boolean {
  if (AI_RUNNING.includes(job.state)) return true;
  return job.state === "queued" && aiReady;
}

export function summaryPanelInterval(
  jobs: MeetingJob[] | undefined,
  pageId: string,
  aiReady: boolean,
): number | false {
  const job = jobs?.find((j) => j.page_id === pageId);
  return job && jobIsActive(job, aiReady) ? ACTIVE_POLL_MS : false;
}

/** Library surfaces: poll while any meeting is processing or a model job is running. */
export function libraryInterval(
  jobs: MeetingJob[] | undefined,
  progress: AiProgress | null | undefined,
  aiReady: boolean,
  running = false,
): number | false {
  if (running || progress) return ACTIVE_POLL_MS;
  return jobs?.some((job) => jobIsActive(job, aiReady))
    ? ACTIVE_POLL_MS
    : false;
}

/** Local AI settings: poll while a download, setup or background job is under way. */
export function localAiInterval(
  progress: AiProgress | null | undefined,
  actionPending: boolean,
): number | false {
  return progress || actionPending ? ACTIVE_POLL_MS : false;
}
