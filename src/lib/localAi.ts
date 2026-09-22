import { invoke } from "./tauri";
import type { MeetingSummary } from "./api";

export interface AiConfig {
  provider: "disabled" | "builtin" | "ollama";
  model_id: string;
  ollama_model: string;
}
export interface AiModel {
  recommended_ram_gb: number;
  tier: string;
  id: string;
  name: string;
  description: string;
  purpose: "chat" | "embedding";
  size: number;
  downloaded: boolean;
}
/** Who asked for the work: "user" is a setup/download the person started here, "worker" is background meeting processing. */
export type AiProgressOrigin = "user" | "worker";
export interface AiProgress {
  label: string;
  model_id: string | null;
  completed: number;
  total: number;
  origin: AiProgressOrigin;
}
/** Background work must not block the settings controls, and is not cancellable from the generic cancel button. */
export const isUserProgress = (progress: AiProgress | null | undefined) =>
  !!progress && progress.origin !== "worker";
export const isWorkerProgress = (progress: AiProgress | null | undefined) =>
  !!progress && progress.origin === "worker";
export interface AiStatus {
  memory_bytes: number | null;
  recommended_model_id: string;
  config: AiConfig;
  models: AiModel[];
  progress: AiProgress | null;
  runtime_available: boolean;
  ready: boolean;
  search_ready: boolean;
  disk_bytes: number;
  recording_paused: boolean;
}
export interface MeetingJob {
  page_id: string;
  title: string;
  state: string;
  error: string | null;
  summary: MeetingSummary | null;
  indexed: boolean;
  client_id: string | null;
  client_name: string | null;
  passage_count: number;
  started_at: number;
}
/** transcript: an indexed passage; notes: a meeting's saved notes; page: the person's own written page. */
export type MeetingSourceKind = "transcript" | "notes" | "page";
export interface MeetingSource {
  id: string;
  page_id: string;
  title: string;
  block_id: string | null;
  timestamp: string | null;
  text: string;
  started_at: number;
  kind?: MeetingSourceKind;
}
export interface MeetingAnswer {
  answer: string;
  sources: MeetingSource[];
  coverage?: { passages_used: number; total_passages: number; meetings_used: number; total_meetings: number } | null;
  /** False when nothing could be supported and the sources are only the closest passages. */
  grounded?: boolean;
  /** How the question was read: dates, client or meeting it pointed at. */
  interpretation?: string[];
}
export const localAi = {
  status: () => invoke<AiStatus>("local_ai_status"),
  configure: (config: AiConfig) =>
    invoke<void>("local_ai_configure", { config }),
  setup: () => invoke<void>("local_ai_setup"),
  download: (id: string) => invoke<void>("local_ai_download", { id }),
  remove: (id: string) =>
    invoke<void>("local_ai_remove", { id, confirmed: true }),
  cancel: () => invoke<void>("local_ai_cancel"),
  cancelRequest: (requestId: string) => invoke<void>("local_ai_cancel", { requestId }),
  jobs: () => invoke<MeetingJob[]>("meeting_ai_jobs"),
  retry: (pageId: string) => invoke<void>("meeting_ai_retry", { pageId }),
  ask: (
    question: string,
    from: number | null,
    until: number | null,
    clientId: string | null = null,
  ) =>
    invoke<MeetingAnswer>("ask_meetings", { question, from, until, clientId }),
  endRecording: () => invoke<void>("ai_end_recording"),
};
export const aiStatusKey = ["local-ai-status"];
export const meetingJobsKey = ["meeting-ai-jobs"];
export function dateRange(
  from: string,
  through: string,
): { from: number | null; until: number | null } {
  const start = from ? new Date(`${from}T00:00:00`).getTime() : null;
  const end = through ? new Date(`${through}T00:00:00`) : null;
  end?.setDate(end.getDate() + 1);
  const until = end?.getTime() ?? null;
  if (
    (start !== null && !Number.isFinite(start)) ||
    (until !== null && !Number.isFinite(until)) ||
    (start !== null && until !== null && start >= until)
  )
    throw new Error("Choose a valid date range.");
  return { from: start, until };
}
