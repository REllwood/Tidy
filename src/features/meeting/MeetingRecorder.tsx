import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import {
  Mic,
  Volume2,
  Square,
  Lock,
  Check,
  Loader2,
  AlertCircle,
  AlertTriangle,
  CircleDot,
  History,
  RefreshCw,
  Bookmark,
  Timer,
} from "lucide-react";
import {
  modelsApi,
  recordingHistoryApi,
  recordingPreferencesApi,
} from "@/lib/api";
import { isTauri } from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";
import { type MeetingPhase } from "./useMeetingFlow";
import { Button } from "@/components/ui/button";

import { useSharedMeetingFlow } from "./MeetingFlowProvider";
import { localAi, aiStatusKey, meetingJobsKey } from "@/lib/localAi";
import { invalidateMeetingQueries } from "./invalidation";
import { languageLabels, type TranscriptionLanguage } from "./transcriptionLanguage";
import {
  controlEnabled,
  countdownCause,
  keepAudioNotice,
  sourceWarning,
} from "./recorderPresentation";

import { AudioLevelMeter } from "./AudioLevelMeter";
import { MeetingNow } from "./MeetingNow";
import { LivePanel } from "./LivePanel";
import { clientOptions } from "./meetingLibrary";

function mmss(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

export function MeetingRecorder() {
  const retention = useQuery({
    queryKey: ["recording-preferences"],
    queryFn: recordingPreferencesApi.get,
  });
  const {
    state,
    language,
    setLanguage,
    start,
    stop,
    reset,
    cancelStart,
    setClient,
    mark,
    keepRecording,
  } = useSharedMeetingFlow();
  const openPage = useUi((s) => s.openPage);
  const { data: models } = useQuery({
    queryKey: ["models"],
    queryFn: modelsApi.list,
  });
  const { data: ai } = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
  });
  const clients = useQuery({ queryKey: meetingJobsKey, queryFn: localAi.jobs });
  const selected = models?.find((m) => m.selected && m.downloaded);
  const keepAudio = keepAudioNotice(state.phase, state.keepAudio, retention.data);
  const blocked = retention.isPending || (!selected && !retention.data);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl px-6 py-10 sm:px-10">
        <p className="page-eyebrow">Capture a conversation</p>
        <h1 className="page-title">Meeting recorder</h1>
        <p className="page-description mb-7">
          Your transcript is saved on this Mac. Keeping the audio is optional.
        </p>

        <p className="mb-5 text-sm text-text-muted">
          {keepAudio
            ? "Meeting audio will be kept for re-transcription."
            : "Audio is discarded after processing. Turn on Keep meeting audio in Settings if you want to re-transcribe later."}
        </p>
        {(state.phase === "idle" || state.phase === "starting") && (
          <div className="workspace-panel p-6 sm:p-7">
            <MeetingNow client={state.client} />
            <SourceRow
              icon={<Mic className="size-4" />}
              name="Microphone"
              sub="Your voice"
            />
            <SourceRow
              icon={<Volume2 className="size-4" />}
              name="System audio"
              sub="Meeting participants and shared audio"
            />
            <div className="mt-4 rounded-lg bg-bg-subtle px-3 py-2 text-note text-text-muted">
              Transcription:{" "}
              {selected ? (
                <b className="text-text">{selected.name} · on-device</b>
              ) : (
                <span className="text-warning">
                  Download a transcription model in Settings. To record without
                  one, turn on Keep meeting audio.
                </span>
              )}
              <br />
              Summaries:{" "}
              {ai?.ready ? (
                <b className="text-text">
                  {ai.config.provider === "builtin"
                    ? "Tidy local AI"
                    : ai.config.ollama_model}{" "}
                  (local)
                </b>
              ) : (
                <span className="text-text-faint">
                  Set up local AI in Settings (transcript still saves)
                </span>
              )}
            </div>
            <label className="mt-4 block space-y-1">
              <span className="text-xs font-medium text-text-faint">
                Client (optional)
              </span>
              <input
                list="meeting-clients"
                disabled={!controlEnabled("start", state.phase) || blocked}
                value={state.client}
                onChange={(e) => setClient(e.target.value)}
                placeholder="e.g. Acme Corp"
                className="w-full rounded-lg border border-border bg-bg px-3 py-2 text-sm outline-none focus:border-brand disabled:opacity-60"
              />
              <datalist id="meeting-clients">
                {clientOptions(clients.data ?? []).map(([id, name]) => (
                  <option key={id} value={name} />
                ))}
              </datalist>
              <span className="block text-xs leading-relaxed text-text-faint">
                Choose an existing client or enter a new name. Meetings for the
                same client can be searched together.
              </span>
            </label>
            <LanguageChoice
              value={language}
              onChange={setLanguage}
              disabled={!controlEnabled("start", state.phase)}
            />
            {state.phase === "starting" && (
              <p
                role="status"
                className="mt-4 flex items-center gap-2 rounded-lg border border-border bg-bg-subtle px-3 py-2.5 text-sm text-text-muted"
              >
                <Loader2 className="size-4 shrink-0 animate-spin" />
                Preparing recording…
              </p>
            )}
            {state.phase === "starting" ? (
              <Button
                className="mt-4 w-full"
                size="lg"
                variant="secondary"
                onClick={() => void cancelStart()}
              >
                Cancel
              </Button>
            ) : (
              <Button
                className="mt-4 w-full"
                size="lg"
                onClick={() => void start()}
                disabled={!controlEnabled("start", state.phase) || blocked}
              >
                {retention.isPending ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <CircleDot className="size-4" />
                )}{" "}
                {retention.isPending
                  ? "Loading recording settings…"
                  : "Start recording"}
              </Button>
            )}
            <p className="mt-3 flex items-start gap-2 text-xs text-text-faint">
              <Lock className="mt-0.5 size-3.5 shrink-0" />
              Everything runs on your Mac. macOS will ask for microphone and
              system audio permission the first time.
            </p>
          </div>
        )}

        {state.phase === "recording" && (
          <div className="workspace-panel p-6 sm:p-7">
            <div className="mb-5 flex items-center gap-3">
              <span
                className="size-3.5 animate-pulse rounded-full bg-rec"
                aria-hidden
              />
              <span className="font-semibold text-rec-text">Recording</span>
            </div>
            <div
              className="mb-1 font-mono text-5xl font-bold tabular-nums"
              role="timer"
            >
              {mmss(state.elapsedMs)}
            </div>
            <div className="mb-5 inline-flex items-center gap-1.5 rounded-full bg-[color-mix(in_srgb,var(--success)_14%,transparent)] px-2.5 py-1 text-xs font-semibold text-success">
              <CircleDot className="size-3" /> On-device · nothing is uploaded
            </div>
            {state.resumed && (
              <p
                role="status"
                className="mb-4 rounded-lg border border-border bg-bg-subtle px-3 py-2.5 text-sm text-text-muted"
              >
                Reconnected to a recording that was already running. Stop it
                here when the meeting finishes.
              </p>
            )}
            <SourceStatus sources={state.sources} />
            <SourceRow
              icon={<Mic className="size-4" />}
              name="Microphone"
              sub={state.sources.mic ? "Capturing" : "Unavailable"}
              meter={
                <AudioLevelMeter
                  label="Microphone"
                  value={state.levels.mic}
                  available={state.sources.mic}
                />
              }
            />
            {state.systemSilent && (
              <p
                role="status"
                className="mb-3 flex items-start gap-2 rounded-lg border border-border bg-bg-subtle px-3 py-2.5 text-sm text-text-muted"
              >
                <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
                No system audio yet. If others are speaking, allow Tidy under
                System Settings, Privacy and Security, Screen and System Audio
                Recording.
              </p>
            )}
            <SourceRow
              icon={<Volume2 className="size-4" />}
              name="System audio"
              sub={state.sources.system ? "Capturing" : "Unavailable"}
              meter={
                <AudioLevelMeter
                  label="System audio"
                  value={state.levels.system}
                  available={state.sources.system}
                />
              }
            />
            {state.countdown && (
              <StopCountdown
                reason={state.countdown.reason}
                deadline={state.countdown.deadline_ms}
                onKeep={() => void keepRecording()}
              />
            )}
            <div className="mt-6 flex gap-2">
              <Button
                variant="secondary"
                size="lg"
                className="shrink-0"
                onClick={() => void mark()}
                title="Mark this moment (Control-Option-M from any app)"
              >
                <Bookmark className="size-4" />
                {state.marks > 0 ? `Marked ${state.marks}` : "Mark moment"}
              </Button>
              <Button
                variant="destructive"
                className="flex-1"
                size="lg"
                disabled={!controlEnabled("stop", state.phase)}
                onClick={() => void stop()}
              >
                <Square className="size-4" /> Stop &amp; transcribe
              </Button>
            </div>
            <p className="mt-2 text-xs text-text-faint">
              Press Control-Option-M in any app to mark a moment. Marked moments
              are listed in the meeting notes.
            </p>
            {selected && <LivePanel lines={state.liveLines} />}
          </div>
        )}

        {["transcribing", "saving"].includes(state.phase) && (
          <ProcessingCard state={state} />
        )}

        {state.phase === "done" && (
          <div className="rounded-[var(--radius-lg)] border border-border bg-surface p-7 text-center shadow-sm">
            <div className="mx-auto mb-3 grid size-12 place-items-center rounded-full bg-success text-white">
              <Check className="size-6" />
            </div>
            <div className="text-lg font-semibold">
              {state.client.trim()
                ? `Filed under ${state.client.trim()}`
                : "Saved to Recording history"}
            </div>
            <p className="mt-1 text-sm text-text-muted">
              Your transcript is saved. Summaries and search will process in the
              meeting library when local AI is enabled.
            </p>
            {state.stopReason && (
              <p className="mt-2 text-sm text-text-muted">{state.stopReason}.</p>
            )}
            <div className="mt-5 flex flex-wrap justify-center gap-2">
              <Button
                onClick={() => state.savedPageId && openPage(state.savedPageId)}
              >
                Open note
              </Button>
              <Button variant="secondary" onClick={reset}>
                New recording
              </Button>
            </div>
          </div>
        )}

        {state.phase === "error" && <RecorderError />}
      </div>
    </div>
  );
}

/** An automatic stop counting down, with the chance to keep recording. */
function StopCountdown({
  reason,
  deadline,
  onKeep,
}: {
  reason: string;
  deadline: number;
  onKeep: () => void;
}) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(timer);
  }, []);
  const seconds = Math.max(0, Math.ceil((deadline - now) / 1000));
  return (
    <div
      role="status"
      className="mt-5 flex items-center gap-3 rounded-lg border border-[color-mix(in_srgb,var(--warning)_40%,transparent)] bg-bg-subtle px-3 py-2.5 text-sm"
    >
      <Timer className="size-4 shrink-0 text-warning" />
      <span className="flex-1">
        {countdownCause(reason)} Stopping in {seconds} s.
      </span>
      <Button size="sm" variant="outline" onClick={onKeep}>
        Keep recording
      </Button>
    </div>
  );
}

function LanguageChoice({
  value,
  onChange,
  disabled,
}: {
  value: TranscriptionLanguage;
  onChange: (value: TranscriptionLanguage) => void;
  disabled?: boolean;
}) {
  return (
    <label className="mt-4 block space-y-1">
      <span className="text-xs font-medium text-text-faint">
        Spoken language
      </span>
      <select
        aria-label="Spoken language"
        disabled={disabled}
        value={value}
        onChange={(e) => onChange(e.target.value as TranscriptionLanguage)}
        className="w-full rounded-lg border border-border bg-bg px-3 py-2 text-sm text-text outline-none focus:border-brand disabled:opacity-60"
      >
        <option value="en">{languageLabels.en}</option>
        <option value="auto">{languageLabels.auto}</option>
      </select>
      <span className="block text-xs leading-relaxed text-text-faint">
        Detect automatically suits meetings in another language. Transcription
        keeps the spoken language; it does not translate.
      </span>
    </label>
  );
}

/** The error card doubles as the recovery path for a meeting that was saved but not transcribed. */
function RecorderError() {
  const { state, reset, language } = useSharedMeetingFlow();
  const qc = useQueryClient();
  const openPage = useUi((s) => s.openPage);
  const setPane = useUi((s) => s.setActivePane);
  const [progress, setProgress] = useState<number | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const pageId = state.savedPageId;
  const retry = useMutation({
    mutationFn: async () => {
      if (!pageId) throw new Error("This recording has no saved page yet.");
      setProgress(null);
      let unlisten: (() => void) | undefined;
      try {
        if (isTauri())
          unlisten = await listen<number>("transcribe-progress", (e) => {
            if (mounted.current) setProgress(e.payload);
          });
        return await recordingHistoryApi.transcribe(
          pageId,
          undefined,
          language,
          useUi.getState().diarizeEnabled,
        );
      } finally {
        unlisten?.();
        invalidateMeetingQueries(qc, pageId);
      }
    },
  });
  return (
    <div className="workspace-panel p-6 sm:p-7">
      <div className="flex items-center gap-2 font-semibold text-danger-c">
        <AlertCircle className="size-5" /> Something went wrong
      </div>
      <p role="alert" className="mt-2 text-sm text-text-muted">
        {state.error}
      </p>
      {pageId && (
        <>
          <p className="mt-3 text-sm text-text-muted">
            The recording was saved. Its audio is kept until a transcript is
            stored, so you can try transcribing it again.
          </p>
          {retry.error && (
            <p role="alert" className="mt-3 text-sm text-danger-c">
              {errorMessage(retry.error)}
            </p>
          )}
          {retry.data && (
            <p role="status" className="mt-3 text-sm text-success">
              {retry.data.applied
                ? "Transcript saved to the meeting note."
                : "A new version was saved in Recording history because the page had changed."}
            </p>
          )}
          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              disabled={retry.isPending}
              onClick={() => retry.mutate()}
            >
              {retry.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <RefreshCw className="size-4" />
              )}
              {retry.isPending
                ? `Transcribing…${progress === null ? "" : ` ${progress}%`}`
                : "Transcribe again"}
            </Button>
            <Button
              variant="secondary"
              onClick={() => setPane({ kind: "history" })}
            >
              <History className="size-4" />
              Open Recording history
            </Button>
            <Button variant="secondary" onClick={() => openPage(pageId)}>
              Open saved recording
            </Button>
          </div>
        </>
      )}
      <Button className="mt-5" variant="secondary" onClick={reset}>
        Try again
      </Button>
    </div>
  );
}

function SourceStatus({
  sources,
}: {
  sources: { mic: boolean; system: boolean };
}) {
  const warning = sourceWarning(sources);
  if (!warning)
    return (
      <p className="mb-4 flex items-center gap-2 rounded-lg border border-border bg-bg-subtle px-3 py-2.5 text-sm text-text-muted">
        <CircleDot className="size-4 shrink-0 text-success" />
        Microphone and system audio are both being captured.
      </p>
    );
  return (
    <p
      role="alert"
      className="mb-4 flex items-start gap-2 rounded-lg border border-warning/40 bg-[color-mix(in_srgb,var(--warning)_12%,transparent)] px-3 py-2.5 text-sm text-text"
    >
      <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
      {warning}
    </p>
  );
}

function SourceRow({
  icon,
  name,
  sub,
  meter,
}: {
  icon: React.ReactNode;
  name: string;
  sub: string;
  meter?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 border-t border-border py-3 first:border-t-0">
      <span className="text-text-muted">{icon}</span>
      <span>
        <span className="block text-sm font-medium">{name}</span>
        <span className="block text-xs text-text-faint">{sub}</span>
      </span>
      {meter}
    </div>
  );
}

function Step({
  status,
  label,
  detail,
  children,
}: {
  status: "done" | "active" | "pending";
  label: string;
  detail?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-3.5 py-3">
      <span
        className={`mt-0.5 grid size-7 shrink-0 place-items-center rounded-full border text-sm ${
          status === "done"
            ? "border-success bg-success text-white"
            : status === "active"
              ? "border-brand text-brand"
              : "border-border-strong text-text-faint"
        }`}
      >
        {status === "done" ? (
          <Check className="size-4" />
        ) : status === "active" ? (
          <Loader2 className="size-4 animate-spin" />
        ) : (
          "·"
        )}
      </span>
      <span className="flex-1">
        <span
          className={`block text-sm font-medium ${status === "pending" ? "text-text-faint" : ""}`}
        >
          {label}
        </span>
        {detail && (
          <span className="block text-xs text-text-faint">{detail}</span>
        )}
        {children}
      </span>
    </div>
  );
}

function ProcessingCard({
  state,
}: {
  state: ReturnType<typeof useSharedMeetingFlow>["state"];
}) {
  const order: MeetingPhase[] = ["transcribing", "saving"];
  const idx = order.indexOf(state.phase);
  const st = (p: MeetingPhase): "done" | "active" | "pending" => {
    const i = order.indexOf(p);
    return i < idx ? "done" : i === idx ? "active" : "pending";
  };
  return (
    <div className="workspace-panel p-6 sm:p-7">
      <div className="mb-2 text-2xs font-semibold uppercase tracking-wide text-text-faint">
        Processing
      </div>
      <Step status="done" label="Captured audio" detail="mic + system" />
      <Step
        status={st("transcribing")}
        label="Transcribing on-device"
        detail="Whisper"
      >
        {state.phase === "transcribing" && (
          <div className="mt-1.5">
            <div className="h-2 overflow-hidden rounded-full bg-bg-subtle">
              <div
                className="h-full rounded-full bg-brand transition-[width]"
                style={{ width: `${state.transcribeProgress}%` }}
                role="progressbar"
                aria-valuenow={state.transcribeProgress}
                aria-valuemin={0}
                aria-valuemax={100}
              />
            </div>
            <div className="mt-1 text-xs text-text-muted">
              {state.transcribeProgress}%
            </div>
          </div>
        )}
      </Step>
      <Step
        status={st("saving")}
        label="Saving transcript and speaker labels"
        detail="AI runs after saving"
      />
      <div className="mt-4 flex items-start gap-2 rounded-lg bg-brand-soft px-3 py-2.5 text-note text-text-muted">
        <Lock className="mt-0.5 size-3.5 shrink-0" />
        Fully local. Your audio and transcript never leave this Mac.
      </div>
    </div>
  );
}
