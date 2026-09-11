import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isTauri } from "@/lib/tauri";
import {
  recordingApi,
  recordingHistoryApi,
  recordingPreferencesApi,
  diarizeApi,
  pagesApi,
  documentsApi,
} from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";
import {
  buildMeetingBlocks,
  assignSpeakers,
  type LabeledSegment,
} from "./meetingDoc";

export type MeetingPhase =
  | "idle"
  | "starting"
  | "recording"
  | "transcribing"
  | "summarizing"
  | "saving"
  | "done"
  | "error";

export interface MeetingState {
  phase: MeetingPhase;
  keepAudio: boolean;
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
  keepAudio: false,
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
    setState((s) => ({ ...initial, client: s.client, phase: "starting" }));
    try {
      if (isTauri()) {
        unlisten.current.push(
          await listen<{ elapsed_ms: number }>("recording-tick", (e) =>
            patch({ elapsedMs: e.payload.elapsed_ms }),
          ),
          await listen<{ mic: number; system: number }>("audio-level", (e) =>
            patch({ levels: e.payload }),
          ),
          await listen<{ mic: boolean; system: boolean }>(
            "recording-sources",
            (e) => patch({ sources: e.payload }),
          ),
          await listen<string>("live-transcript", (e) =>
            patch({ liveTranscript: e.payload }),
          ),
        );
        const keepAudio = await recordingApi.start();
        patch({ phase: "recording", keepAudio });
      } else {
        // Browser/mock: self-drive timer + fake levels so the UI is demoable.
        const keepAudio = await recordingApi.start();
        patch({ phase: "recording", keepAudio });
        const t0 = Date.now();
        mockTimer.current = setInterval(() => {
          patch({
            elapsedMs: Date.now() - t0,
            levels: {
              mic: Math.random() < 0.2 ? 0 : 0.002 + Math.random() * 0.04,
              system: Math.random() < 0.3 ? 0 : 0.001 + Math.random() * 0.03,
            },
          });
        }, 150);
      }
    } catch (e) {
      cleanup();
      busy.current = false;
      patch({ phase: "error", error: errorMessage(e) });
    }
  }, [cleanup]);

  const stopping = useRef(false);
  const stop = useCallback(async () => {
    if (stopping.current) return;
    stopping.current = true;
    cleanup();
    let progressUnlisten: UnlistenFn | null = null;
    let savedPage: string | null = null;
    try {
      patch({ phase: "transcribing", transcribeProgress: 0 });
      if (isTauri()) {
        progressUnlisten = await listen<number>("transcribe-progress", (e) =>
          patch({ transcribeProgress: e.payload }),
        );
      }
      const rec = await recordingApi.stop(clientRef.current);
      if (!rec.page_id)
        throw new Error(
          "Recording saved, but its meeting page could not be opened. Check Recording history.",
        );
      const pageId = rec.page_id;
      savedPage = pageId;
      patch({ savedPageId: pageId });
      const now = new Date();
      await pagesApi.rename(
        pageId,
        `Meeting ${now.toLocaleDateString("en-AU")} ${now.toLocaleTimeString("en-AU", { hour: "2-digit", minute: "2-digit" })}`,
      );
      await qc.invalidateQueries({ queryKey: ["recording-history"] });
      const result = await recordingHistoryApi.transcribe(pageId);
      const segments = result.segments;
      const bodyJson = result.body_json;
      patch({ transcribeProgress: 100, phase: "saving" });

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

      if (result.applied && labeled !== segments) {
        await documentsApi.updateIfUnchanged(
          pageId,
          bodyJson,
          JSON.stringify(buildMeetingBlocks(labeled, null, false)),
        );
      }
      qc.invalidateQueries();
      patch({ phase: "done", savedPageId: pageId });
    } catch (e) {
      patch({ phase: "error", error: errorMessage(e) });
    } finally {
      if (savedPage) {
        try {
          await recordingPreferencesApi.finish(savedPage);
        } catch (error) {
          patch({
            phase: "error",
            error: `Temporary audio could not be cleared: ${errorMessage(error)}`,
          });
        }
      }
      progressUnlisten?.();
      stopping.current = false;
      busy.current = false;
      await qc.invalidateQueries();
    }
  }, [cleanup, qc, diarizeEnabled]);

  const reset = useCallback(() => {
    cleanup();
    busy.current = false;
    clientRef.current = "";
    setState(initial);
  }, [cleanup]);

  return { state, start, stop, reset, setClient };
}
