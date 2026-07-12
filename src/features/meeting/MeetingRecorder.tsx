import { useQuery } from "@tanstack/react-query";
import {
  Mic,
  Volume2,
  Square,
  Lock,
  Check,
  Loader2,
  AlertCircle,
  CircleDot,
} from "lucide-react";
import { modelsApi, ollamaApi } from "@/lib/api";
import { useUi } from "@/store/ui";
import { useMeetingFlow, type MeetingPhase } from "./useMeetingFlow";
import { Button } from "@/components/ui/button";

function mmss(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

function LevelMeter({ label, value }: { label: string; value: number }) {
  const bars = 5;
  return (
    <span className="ml-auto flex h-6 items-end gap-[3px]" aria-label={`${label} level`}>
      {Array.from({ length: bars }).map((_, i) => {
        const active = value * bars > i;
        return (
          <i
            key={i}
            className="w-1 rounded-sm bg-brand transition-[height]"
            style={{ height: active ? `${8 + i * 4}px` : "4px", opacity: active ? 1 : 0.3 }}
          />
        );
      })}
    </span>
  );
}

export function MeetingRecorder() {
  const { state, start, stop, reset, setClient } = useMeetingFlow();
  const openPage = useUi((s) => s.openPage);
  const { data: models } = useQuery({ queryKey: ["models"], queryFn: modelsApi.list });
  const { data: ollama } = useQuery({ queryKey: ["ollama-status"], queryFn: ollamaApi.status });
  const selected = models?.find((m) => m.selected && m.downloaded);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-xl px-8 py-12">
        <h1 className="mb-6 flex items-center gap-2.5 text-xl font-semibold">
          <span className="grid size-8 place-items-center rounded-lg bg-brand-soft text-brand">
            <Mic className="size-[18px]" />
          </span>
          Meeting recorder
        </h1>

        {state.phase === "idle" && (
          <div className="rounded-[var(--radius-lg)] border border-border bg-surface p-7 shadow-sm">
            <SourceRow icon={<Mic className="size-4" />} name="Microphone" sub="Your voice" />
            <SourceRow
              icon={<Volume2 className="size-4" />}
              name="System audio"
              sub="Other participants (ScreenCaptureKit)"
            />
            <div className="mt-4 rounded-lg bg-bg-subtle px-3 py-2 text-note text-text-muted">
              Transcription:{" "}
              {selected ? (
                <b className="text-text">{selected.name} · on-device</b>
              ) : (
                <span className="text-warning">
                  No Whisper model yet. Download one in Settings.
                </span>
              )}
              <br />
              Summaries:{" "}
              {ollama?.available ? (
                <b className="text-text">{ollama.models[0] ?? "Ollama"} (local)</b>
              ) : (
                <span className="text-text-faint">Ollama not detected (transcript still saves)</span>
              )}
            </div>
            <label className="mt-4 block space-y-1">
              <span className="text-xs font-medium text-text-faint">
                Client (optional). Files the note under them.
              </span>
              <input
                value={state.client}
                onChange={(e) => setClient(e.target.value)}
                placeholder="e.g. Acme Corp"
                className="w-full rounded-lg border border-border bg-bg px-3 py-2 text-sm outline-none focus:border-brand"
              />
            </label>
            <Button className="mt-4 w-full" size="lg" onClick={start} disabled={!selected}>
              <CircleDot className="size-4" /> Start recording
            </Button>
            <p className="mt-3 flex items-start gap-2 text-xs text-text-faint">
              <Lock className="mt-0.5 size-3.5 shrink-0" />
              Everything runs on your Mac. macOS will ask for microphone and screen-recording
              permission the first time.
            </p>
          </div>
        )}

        {state.phase === "recording" && (
          <div className="rounded-[var(--radius-lg)] border border-border bg-surface p-7 shadow-sm">
            <div className="mb-5 flex items-center gap-3">
              <span className="size-3.5 animate-pulse rounded-full bg-rec" aria-hidden />
              <span className="font-semibold text-rec-text">Recording</span>
            </div>
            <div className="mb-1 font-mono text-5xl font-bold tabular-nums" role="timer">
              {mmss(state.elapsedMs)}
            </div>
            <div className="mb-6 inline-flex items-center gap-1.5 rounded-full bg-[color-mix(in_srgb,var(--success)_14%,transparent)] px-2.5 py-1 text-xs font-semibold text-success">
              <CircleDot className="size-3" /> On-device · nothing is uploaded
            </div>
            <SourceRow
              icon={<Mic className="size-4" />}
              name="Microphone"
              sub={state.sources.mic ? "Capturing" : "Unavailable"}
              meter={<LevelMeter label="Mic" value={state.levels.mic} />}
            />
            <SourceRow
              icon={<Volume2 className="size-4" />}
              name="System audio"
              sub={state.sources.system ? "Capturing" : "Unavailable"}
              meter={<LevelMeter label="System" value={state.levels.system} />}
            />
            <Button variant="destructive" className="mt-6 w-full" size="lg" onClick={stop}>
              <Square className="size-4" /> Stop &amp; transcribe
            </Button>
            {state.liveTranscript && (
              <div className="mt-5 border-t border-border pt-4">
                <div className="mb-1.5 text-2xs font-semibold uppercase tracking-wide text-text-faint">
                  Live preview
                </div>
                <p className="max-h-32 overflow-y-auto text-sm leading-relaxed text-text-muted">
                  {state.liveTranscript}
                </p>
              </div>
            )}
          </div>
        )}

        {["transcribing", "summarizing", "saving"].includes(state.phase) && (
          <ProcessingCard state={state} />
        )}

        {state.phase === "done" && (
          <div className="rounded-[var(--radius-lg)] border border-border bg-surface p-7 text-center shadow-sm">
            <div className="mx-auto mb-3 grid size-12 place-items-center rounded-full bg-success text-white">
              <Check className="size-6" />
            </div>
            <div className="text-lg font-semibold">
              {state.client.trim() ? `Filed under ${state.client.trim()}` : "Saved to Meeting Notes"}
            </div>
            <p className="mt-1 text-sm text-text-muted">
              Your transcript{state.ollamaUsed ? " and summary are" : " is"} ready.
            </p>
            <div className="mt-5 flex justify-center gap-2">
              <Button onClick={() => state.savedPageId && openPage(state.savedPageId)}>
                Open note
              </Button>
              <Button variant="secondary" onClick={reset}>
                New recording
              </Button>
            </div>
          </div>
        )}

        {state.phase === "error" && (
          <div className="rounded-[var(--radius-lg)] border border-border bg-surface p-7 shadow-sm">
            <div className="flex items-center gap-2 font-semibold text-danger-c">
              <AlertCircle className="size-5" /> Something went wrong
            </div>
            <p className="mt-2 text-sm text-text-muted">{state.error}</p>
            <Button className="mt-5" variant="secondary" onClick={reset}>
              Try again
            </Button>
          </div>
        )}
      </div>
    </div>
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
        <span className={`block text-sm font-medium ${status === "pending" ? "text-text-faint" : ""}`}>
          {label}
        </span>
        {detail && <span className="block text-xs text-text-faint">{detail}</span>}
        {children}
      </span>
    </div>
  );
}

function ProcessingCard({ state }: { state: ReturnType<typeof useMeetingFlow>["state"] }) {
  const order: MeetingPhase[] = ["transcribing", "summarizing", "saving"];
  const idx = order.indexOf(state.phase);
  const st = (p: MeetingPhase): "done" | "active" | "pending" => {
    const i = order.indexOf(p);
    return i < idx ? "done" : i === idx ? "active" : "pending";
  };
  return (
    <div className="rounded-[var(--radius-lg)] border border-border bg-surface p-7 shadow-sm">
      <div className="mb-2 text-2xs font-semibold uppercase tracking-wide text-text-faint">
        Processing
      </div>
      <Step status="done" label="Captured audio" detail="mic + system" />
      <Step status={st("transcribing")} label="Transcribing on-device" detail="Whisper">
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
            <div className="mt-1 text-xs text-text-muted">{state.transcribeProgress}%</div>
          </div>
        )}
      </Step>
      <Step
        status={st("summarizing")}
        label="Summarising"
        detail={state.ollamaUsed ? "local LLM" : "Ollama (skipped if absent)"}
      />
      <Step status={st("saving")} label="Saving note" detail="→ Meeting Notes" />
      <div className="mt-4 flex items-start gap-2 rounded-lg bg-brand-soft px-3 py-2.5 text-note text-text-muted">
        <Lock className="mt-0.5 size-3.5 shrink-0" />
        Fully local. Your audio and transcript never leave this Mac.
      </div>
    </div>
  );
}
