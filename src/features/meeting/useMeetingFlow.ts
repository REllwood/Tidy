import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { isTauri } from "@/lib/tauri";
import {
  recordingApi,
  recordingHistoryApi,
  recordingPreferencesApi,
  recordingHistoryKey,
  pagesApi,
  meetingHudApi,
  type MeetingHud,
} from "@/lib/api";
import { localAi } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";
import { invalidateMeetingQueries } from "./invalidation";
import {
  readTranscriptionLanguage,
  writeTranscriptionLanguage,
  type TranscriptionLanguage,
} from "./transcriptionLanguage";

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
  /** True when the recorder adopted a recording that was already running (webview reload). */
  resumed: boolean;
  /** Moments marked during this recording. */
  marks: number;
  /** Why Tidy stopped the recording itself, if it did. */
  stopReason: string | null;
  /** An automatic stop counting down, which "Keep recording" cancels. */
  countdown: { reason: string; deadline_ms: number } | null;
  /** System audio has been completely silent, which can mean it is not allowed. */
  systemSilent: boolean;
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
  resumed: false,
  marks: 0,
  stopReason: null,
  countdown: null,
  systemSilent: false,
};

const quiet = { mic: 0, system: 0 };

/** Releasing the AI gate is best effort: it must never mask the real failure. */
async function releaseAiGate() {
  try {
    await localAi.endRecording();
  } catch (error) {
    console.error("could not release the local AI recording gate", error);
  }
}

export function useMeetingFlow() {
  const [state, setState] = useState<MeetingState>(initial);
  const patch = (p: Partial<MeetingState>) => setState((s) => ({ ...s, ...p }));
  const qc = useQueryClient();
  const diarizeEnabled = useUi((s) => s.diarizeEnabled);
  const unlisten = useRef<UnlistenFn[]>([]);
  const mockTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const busy = useRef(false);
  /** The native recorder is running, so stop() has something to stop. */
  const live = useRef(false);
  /** Bumped by reset(): a start still in flight belongs to an older attempt. */
  const generation = useRef(0);
  /** Resolves once the in-flight start() has settled, so stop() can wait for it. */
  const starting = useRef<Promise<void> | null>(null);
  // Optional client to file the meeting under (read in the async stop() flow).
  const clientRef = useRef("");
  const setClient = useCallback((c: string) => {
    clientRef.current = c;
    setState((s) => ({ ...s, client: c }));
  }, []);

  const [language, setLanguageState] = useState<TranscriptionLanguage>(() =>
    readTranscriptionLanguage(),
  );
  const languageRef = useRef(language);
  const setLanguage = useCallback((value: TranscriptionLanguage) => {
    languageRef.current = value;
    setLanguageState(value);
    writeTranscriptionLanguage(value);
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

  /**
   * Each handle is stored as soon as it exists: if one subscription rejects,
   * the ones already made are still tracked and torn down by cleanup().
   */
  const subscribe = useCallback(async () => {
    if (!isTauri()) return;
    unlisten.current.push(
      await listen<{ elapsed_ms: number }>("recording-tick", (e) =>
        patch({ elapsedMs: e.payload.elapsed_ms }),
      ),
    );
    unlisten.current.push(
      await listen<{ mic: number; system: number }>("audio-level", (e) =>
        patch({ levels: e.payload }),
      ),
    );
    unlisten.current.push(
      await listen<{ mic: boolean; system: boolean }>("recording-sources", (e) =>
        patch({ sources: e.payload }),
      ),
    );
    unlisten.current.push(
      await listen<string>("live-transcript", (e) =>
        patch({ liveTranscript: e.payload }),
      ),
    );
    unlisten.current.push(
      await listen("system-audio-silent", () => patch({ systemSilent: true })),
    );
    unlisten.current.push(
      await listen<{ count: number }>("moment-marked", (e) =>
        patch({ marks: e.payload.count }),
      ),
    );
    unlisten.current.push(
      await listen<MeetingHud>("meeting-hud", (e) =>
        patch({
          countdown:
            e.payload.kind === "countdown"
              ? { reason: e.payload.reason, deadline_ms: e.payload.deadline_ms }
              : null,
        }),
      ),
    );
  }, []);

  const startMockMeters = useCallback(() => {
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
  }, []);

  const start = useCallback(async () => {
    if (busy.current) return; // guard against double-start (duplicate listeners)
    busy.current = true;
    const attempt = ++generation.current;
    // Reset everything except the client the user typed before hitting record.
    setState((s) => ({ ...initial, client: s.client, phase: "starting" }));
    const run = async () => {
      try {
        await subscribe();
        const keepAudio = await recordingApi.start();
        if (attempt !== generation.current) {
          // Cancelled while starting: tear the native recorder back down.
          await recordingApi.stop(clientRef.current).catch(() => undefined);
          await releaseAiGate();
          cleanup();
          return;
        }
        live.current = true;
        patch({ phase: "recording", keepAudio });
        if (!isTauri()) startMockMeters();
      } catch (e) {
        cleanup();
        live.current = false;
        busy.current = false;
        await releaseAiGate();
        if (attempt === generation.current)
          patch({ phase: "error", error: errorMessage(e) });
      }
    };
    starting.current = run();
    await starting.current;
    starting.current = null;
  }, [cleanup, startMockMeters, subscribe]);

  /**
   * Adopt a recording that is already running in the core, so reloading the
   * webview mid-meeting reconnects the timer, meters and banner instead of
   * leaving an invisible recorder behind.
   */
  const reconcile = useCallback(async () => {
    if (busy.current || live.current) return;
    try {
      if (!(await recordingApi.isRecording())) return;
      if (busy.current || live.current) return;
      busy.current = true;
      live.current = true;
      generation.current++;
      // The core does not replay the retention flag it returned at start, so
      // fall back to the saved preference, which is what it was read from.
      const keepAudio = await recordingPreferencesApi
        .get()
        .catch(() => false);
      setState((s) => ({
        ...initial,
        client: s.client,
        phase: "recording",
        keepAudio,
        resumed: true,
      }));
      await subscribe();
      if (!isTauri()) startMockMeters();
    } catch (e) {
      console.error("could not reconnect to the running recording", e);
    }
  }, [startMockMeters, subscribe]);

  const stopping = useRef(false);
  /** `reason` is set when Tidy stops the recording itself. */
  const stop = useCallback(async (reason?: string) => {
    if (stopping.current) return;
    stopping.current = true;
    // (i) A stop pressed during "starting" waits for the start to settle first.
    await starting.current?.catch(() => undefined);
    if (!live.current) {
      stopping.current = false;
      return;
    }
    live.current = false;
    cleanup();
    let progressUnlisten: UnlistenFn | null = null;
    let savedPage: string | null = null;
    try {
      patch({
        phase: "transcribing",
        transcribeProgress: 0,
        levels: quiet,
        countdown: null,
        stopReason: reason ?? null,
      });
      if (isTauri()) {
        progressUnlisten = await listen<number>("transcribe-progress", (e) =>
          patch({ transcribeProgress: e.payload }),
        );
      }
      const rec = await recordingApi.stop(clientRef.current, reason);
      if (!rec.page_id)
        throw new Error(
          "Recording saved, but its meeting page could not be opened. Check Recording history.",
        );
      const pageId = rec.page_id;
      savedPage = pageId;
      patch({ savedPageId: pageId });
      const now = new Date();
      // Named after the calendar event when there was one.
      await pagesApi.rename(
        pageId,
        rec.title?.trim() ||
          `Meeting ${now.toLocaleDateString("en-AU")} ${now.toLocaleTimeString("en-AU", { hour: "2-digit", minute: "2-digit" })}`,
      );
      await qc.invalidateQueries({ queryKey: recordingHistoryKey });
      // Each side of the call is transcribed on its own, and the other side
      // split into speakers when that is switched on.
      await recordingHistoryApi.transcribe(
        pageId,
        undefined,
        languageRef.current,
        diarizeEnabled,
      );
      patch({ transcribeProgress: 100, phase: "saving" });
      patch({ phase: "done", savedPageId: pageId });
      // Only now is the transcript safe, so temporary audio may be cleared.
      // A failure here is reported without pulling the flow out of "done".
      try {
        await recordingPreferencesApi.finish(pageId);
      } catch (error) {
        toast.error(
          `Temporary audio could not be cleared: ${errorMessage(error)}`,
        );
      }
    } catch (e) {
      // Keep the saved page so the recorder can offer another transcription.
      patch({
        phase: "error",
        error: errorMessage(e),
        savedPageId: savedPage,
        levels: quiet,
      });
      await releaseAiGate();
    } finally {
      progressUnlisten?.();
      stopping.current = false;
      busy.current = false;
      invalidateMeetingQueries(qc, savedPage);
    }
  }, [cleanup, qc, diarizeEnabled]);

  const reset = useCallback(() => {
    generation.current++;
    cleanup();
    busy.current = false;
    live.current = false;
    clientRef.current = "";
    setState(initial);
  }, [cleanup]);

  /** Mark this moment in the recording. */
  const mark = useCallback(async () => {
    try {
      await recordingApi.mark();
      if (!isTauri()) setState((s) => ({ ...s, marks: s.marks + 1 }));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }, []);

  /** Cancel an automatic stop that is counting down. */
  const keepRecording = useCallback(async () => {
    patch({ countdown: null });
    await meetingHudApi.respond("keep").catch((e) => toast.error(errorMessage(e)));
  }, []);

  /** Give up on a start that has not finished yet, and hand the AI gate back. */
  const cancelStart = useCallback(async () => {
    reset();
    await releaseAiGate();
  }, [reset]);

  return {
    state,
    language,
    setLanguage,
    start,
    stop,
    reset,
    cancelStart,
    reconcile,
    setClient,
    mark,
    keepRecording,
  };
}
