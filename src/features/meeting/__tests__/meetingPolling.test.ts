import { describe, expect, it } from "vitest";
import {
  ACTIVE_POLL_MS,
  isMeetingPage,
  jobIsActive,
  libraryInterval,
  localAiInterval,
  recordingHistoryInterval,
  recordingPanelInterval,
  summaryPanelInterval,
} from "../polling";
import { meetingInvalidationKeys } from "../invalidation";
import type { SavedRecording } from "@/lib/api";
import type { AiProgress, MeetingJob } from "@/lib/localAi";

const recording = (
  pageId: string,
  state: string,
  extra: Partial<SavedRecording> = {},
): SavedRecording => ({
  retain_audio: false,
  id: `rec-${pageId}`,
  page_id: pageId,
  title: "Meeting",
  client: null,
  started_at: 0,
  duration_ms: 60000,
  audio_path: "/tmp/a.wav",
  audio_available: true,
  audio_temporary: true,
  model_used: null,
  transcript_state: state,
  transcript_error: null,
  ...extra,
});

const job = (pageId: string, state: string): MeetingJob => ({
  page_id: pageId,
  title: "Meeting",
  state,
  error: null,
  summary: null,
  indexed: false,
  client_id: null,
  client_name: null,
  passage_count: 0,
  started_at: 0,
});

const progress = (origin: "user" | "worker"): AiProgress => ({
  label: "Working",
  model_id: null,
  completed: 1,
  total: 10,
  origin,
});

describe("meeting polling", () => {
  it("leaves an ordinary document page completely idle", () => {
    const rows = [recording("meeting", "saved")];
    expect(isMeetingPage(rows, "notes")).toBe(false);
    expect(recordingPanelInterval(rows, "notes", false)).toBe(false);
    expect(summaryPanelInterval([job("meeting", "queued")], "notes", true)).toBe(
      false,
    );
  });

  it("polls the recording panel only while this page transcribes", () => {
    const rows = [recording("meeting", "transcribing")];
    expect(recordingPanelInterval(rows, "meeting", false)).toBe(ACTIVE_POLL_MS);
    expect(
      recordingPanelInterval([recording("meeting", "saved")], "meeting", false),
    ).toBe(false);
    // A run started in this tab polls before the list has caught up.
    expect(
      recordingPanelInterval([recording("meeting", "saved")], "meeting", true),
    ).toBe(ACTIVE_POLL_MS);
  });

  it("polls the history list only while some meeting transcribes", () => {
    expect(recordingHistoryInterval([recording("a", "saved")])).toBe(false);
    expect(
      recordingHistoryInterval([
        recording("a", "saved"),
        recording("b", "transcribing"),
      ]),
    ).toBe(ACTIVE_POLL_MS);
    expect(recordingHistoryInterval(undefined)).toBe(false);
  });

  it("does not wait forever on a queued job when local AI is switched off", () => {
    expect(jobIsActive(job("a", "queued"), false)).toBe(false);
    expect(jobIsActive(job("a", "queued"), true)).toBe(true);
    expect(jobIsActive(job("a", "summarising"), false)).toBe(true);
    expect(jobIsActive(job("a", "ready"), true)).toBe(false);
    expect(summaryPanelInterval([job("a", "ready")], "a", true)).toBe(false);
    expect(summaryPanelInterval([job("a", "indexing")], "a", false)).toBe(
      ACTIVE_POLL_MS,
    );
  });

  it("keeps the library ticking while work of any kind is running", () => {
    expect(libraryInterval([job("a", "ready")], null, true)).toBe(false);
    expect(libraryInterval([job("a", "ready")], progress("worker"), true)).toBe(
      ACTIVE_POLL_MS,
    );
    expect(libraryInterval([job("a", "ready")], null, true, true)).toBe(
      ACTIVE_POLL_MS,
    );
    expect(libraryInterval([job("a", "summarising")], null, true)).toBe(
      ACTIVE_POLL_MS,
    );
  });

  it("only polls local AI settings while something is downloading or running", () => {
    expect(localAiInterval(null, false)).toBe(false);
    expect(localAiInterval(null, true)).toBe(ACTIVE_POLL_MS);
    expect(localAiInterval(progress("worker"), false)).toBe(ACTIVE_POLL_MS);
  });
});

describe("meeting invalidation", () => {
  it("names the affected caches instead of resetting everything", () => {
    const keys = meetingInvalidationKeys("page-1").map((k) => JSON.stringify(k));
    expect(keys).toContain(JSON.stringify(["pages"]));
    expect(keys).toContain(JSON.stringify(["recording-history"]));
    expect(keys).toContain(JSON.stringify(["meeting-ai-jobs"]));
    expect(keys).toContain(JSON.stringify(["local-ai-status"]));
    expect(keys).toContain(JSON.stringify(["database"]));
    expect(keys).toContain(JSON.stringify(["database-by-id"]));
    expect(keys).toContain(JSON.stringify(["document", "page-1"]));
    expect(keys).toContain(JSON.stringify(["transcript-versions", "page-1"]));
  });

  it("skips the page-scoped caches when no page was saved", () => {
    const keys = meetingInvalidationKeys(null).map((k) => JSON.stringify(k));
    expect(keys.some((k) => k.includes("document"))).toBe(false);
    expect(keys).toContain(JSON.stringify(["pages"]));
  });
});
