import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "@/lib/tauri";
import {
  recordingApi,
  transcribeApi,
  diarizeApi,
  ollamaApi,
  pagesApi,
  ingestApi,
  modelsApi,
  type MeetingSummary,
} from "@/lib/api";
import { useUi } from "@/store/ui";
import { buildMeetingBlocks, assignSpeakers, type LabeledSegment } from "./meetingDoc";

export type MeetingPhase =
  | "idle"
  | "recording"
  | "transcribing"
  | "summarizing"
  | "saving"
  | "done"
  | "error";

export interface MeetingState {
  phase: MeetingPhase;
  elapsedMs: number;
  levels: { mic: number; system: number };
  sources: { mic: boolean; system: boolean };
  transcribeProgress: number;
  ollamaUsed: boolean;
  liveTranscript: string;
  savedPageId: string | null;
  client: string;
  error: string | null;
}

const initial: MeetingState = {
  phase: "idle",
  elapsedMs: 0,
  levels: { mic: 0, system: 0 },
  sources: { mic: true, system: true },
  transcribeProgress: 0,
  ollamaUsed: false,
  liveTranscript: "",
  savedPageId: null,
  client: "",
  error: null,
};

export function useMeetingFlow() {
  const [state, setState] = useState<MeetingState>(initial);
  const patch = (p: Partial<MeetingState>) => setState((s) => ({ ...s, ...p }));
  const qc = useQueryClient();
  const diarizeEnabled = useUi((s) => s.diarizeEnabled);
  const unlisten = useRef<UnlistenFn[]>([]);
  const mockTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const busy = useRef(false);
  // Optional client to file the meeting under (read in the async stop() flow).
  const clientRef = useRef("");
  const setClient = useCallback((c: string) => {
    clientRef.current = c;
    setState((s) => ({ ...s, client: c }));
  }, []);

  const cleanup = useCallback(() => {
    unlisten.current.forEach((fn) => fn());
    unlisten.current = [];
    if (mockTimer.current) {
      clearInterval(mockTimer.current);
      mockTimer.current = null;
    }
  }, []);

  useEffect(() => () => cleanup(), [cleanup]);

  const start = useCallback(async () => {
    if (busy.current) return; // guard against double-start (duplicate listeners)
    busy.current = true;
    // Reset everything except the client the user typed before hitting record.
    setState((s) => ({ ...initial, client: s.client, phase: "recording" }));
    try {
      if (isTauri()) {
        unlisten.current.push(
          await listen<{ elapsed_ms: number }>("recording-tick", (e) =>
            patch({ elapsedMs: e.payload.elapsed_ms }),
          ),
          await listen<{ mic: number; system: number }>("audio-level", (e) =>
            patch({ levels: e.payload }),
          ),
          await listen<{ mic: boolean; system: boolean }>("recording-sources", (e) =>
            patch({ sources: e.payload }),
          ),
          await listen<string>("live-transcript", (e) =>
            patch({ liveTranscript: e.payload }),
          ),
        );
        await recordingApi.start();
      } else {
        // Browser/mock: self-drive timer + fake levels so the UI is demoable.
        await recordingApi.start();
        const t0 = Date.now();
        mockTimer.current = setInterval(() => {
          patch({
            elapsedMs: Date.now() - t0,
            levels: { mic: 0.3 + Math.random() * 0.4, system: 0.2 + Math.random() * 0.3 },
          });
        }, 150);
      }
    } catch (e) {
      patch({ phase: "error", error: String(e) });
    }
  }, []);

  const stop = useCallback(async () => {
    cleanup();
    try {
      patch({ phase: "transcribing", transcribeProgress: 0 });
      let progressUnlisten: UnlistenFn | null = null;
      if (isTauri()) {
        progressUnlisten = await listen<number>("transcribe-progress", (e) =>
          patch({ transcribeProgress: e.payload }),
        );
      }
      const rec = await recordingApi.stop();
      const modelUsed = (await modelsApi.list()).find((m) => m.selected)?.name ?? null;
      const segments = await transcribeApi.run(rec.audio_path);
      progressUnlisten?.();
      patch({ transcribeProgress: 100 });

      // Summarize if Ollama is available; otherwise save transcript-only.
      patch({ phase: "summarizing" });
      let summary: MeetingSummary | null = null;
      let ollamaUsed = false;
      try {
        const status = await ollamaApi.status();
        if (status.available) {
          const text = segments.map((s) => s.text).join(" ");
          summary = await ollamaApi.summarize(text);
          ollamaUsed = true;
        }
      } catch {
        summary = null;
      }
      patch({ ollamaUsed });

      // Speaker diarization (optional; native-only, models must be installed).
      let labeled: LabeledSegment[] = segments;
      if (diarizeEnabled) {
        try {
          if (await diarizeApi.available()) {
            const speakers = await diarizeApi.run(rec.audio_path);
            labeled = assignSpeakers(segments, speakers);
          }
        } catch (e) {
          console.error("diarization failed", e);
        }
      }

      patch({ phase: "saving" });
      const pageId = await fileMeeting(labeled, summary, ollamaUsed, clientRef.current);
      await recordingApi
        .record(pageId, rec.duration_ms, rec.audio_path, modelUsed)
        .catch((e) => console.error("record meeting failed", e));
      qc.invalidateQueries();
      patch({ phase: "done", savedPageId: pageId });
    } catch (e) {
      patch({ phase: "error", error: String(e) });
    }
  }, [cleanup, qc]);

  const reset = useCallback(() => {
    cleanup();
    busy.current = false;
    clientRef.current = "";
    setState(initial);
  }, [cleanup]);

  return { state, start, stop, reset, setClient };
}

/**
 * File the meeting through the shared `ingest_note` pipeline: it creates the
 * record page, files it under the client (creating one if needed), links it,
 * and turns action items into Task rows, while we keep the rich diarized
 * transcript by passing it as the pre-rendered `bodyJson`.
 */
async function fileMeeting(
  segments: LabeledSegment[],
  summary: MeetingSummary | null,
  ollamaUsed: boolean,
  client: string,
): Promise<string> {
  const now = new Date();
  const title = `Meeting ${now.toLocaleDateString()} ${now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
  const blocks = buildMeetingBlocks(segments, summary, ollamaUsed);

  const res = await ingestApi.ingestNote({
    rawText: segments.map((s) => s.text).join(" "),
    bodyJson: JSON.stringify(blocks),
    title,
    clientHint: client.trim() || undefined,
    actionItems: summary?.action_items ?? [],
  });
  await pagesApi.setIcon(res.page_id, "🎙️").catch(() => {});

  // With no client, keep the old default of grouping under "Meeting Notes".
  if (!client.trim()) {
    const pages = await pagesApi.list();
    const parent = pages.find((p) => p.title.toLowerCase() === "meeting notes")?.id;
    if (parent) await pagesApi.move(res.page_id, parent, 999).catch(() => {});
  }
  return res.page_id;
}
