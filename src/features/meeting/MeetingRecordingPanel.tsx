import { useEffect, useRef, useState } from "react";
import { useUi } from "@/store/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { AudioLines, ChevronDown, Loader2 } from "lucide-react";
import {
  documentsApi,
  modelsApi,
  recordingHistoryApi,
  type RetranscriptionResult,
  type SavedRecording,
} from "@/lib/api";
import { meetingJobsKey } from "@/lib/localAi";
import { isTauri } from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { recordingHistoryKey } from "@/lib/api";
import { transcriptText } from "./transcriptText";
import { recordingPanelInterval } from "./polling";

/** How long the cancel button stays quiet before it can be pressed again. */
const CANCEL_RETRY_MS = 5000;

function elapsedLabel(since: number | null, tick: number) {
  if (since === null) return "";
  const seconds = Math.max(0, Math.floor((tick - since) / 1000));
  return ` ${seconds}s`;
}

export function MeetingRecordingPanel({
  pageId,
  beforeTranscribe,
  onResult,
  onBusy,
}: {
  pageId: string;
  beforeTranscribe: () => Promise<void>;
  onResult: (r: RetranscriptionResult) => void;
  onBusy: (busy: boolean) => void;
}) {
  const qc = useQueryClient();
  const [running, setRunning] = useState(false);
  // This panel renders on every document page, so an ordinary note must not
  // poll: the list is read once and only ticks while this meeting is working.
  const history = useQuery({
    queryKey: recordingHistoryKey,
    queryFn: recordingHistoryApi.list,
    staleTime: 30_000,
    refetchInterval: (query) =>
      recordingPanelInterval(
        query.state.data as SavedRecording[] | undefined,
        pageId,
        running,
      ),
  });
  const meeting = history.data?.find((m) => m.page_id === pageId);
  const versions = useQuery({
    queryKey: ["transcript-versions", pageId],
    queryFn: () => recordingHistoryApi.versions(pageId),
    enabled: !!meeting,
  });
  const models = useQuery({
    queryKey: ["models"],
    queryFn: modelsApi.list,
    enabled: !!meeting,
    staleTime: 30_000,
  });
  const [model, setModel] = useState("");
  const [language, setLanguage] = useState("en");
  const [version, setVersion] = useState("");
  const [progress, setProgress] = useState<number | null>(null);
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const [cancelledAt, setCancelledAt] = useState<number | null>(null);
  const [lastCancelAt, setLastCancelAt] = useState<number | null>(null);
  const [tick, setTick] = useState(() => Date.now());
  // Folded away once a transcript is saved: the notes come first then.
  const [open, setOpen] = useState<boolean | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  // Drives the "Cancelling… 12s" counter and re-enables the button after a wait.
  useEffect(() => {
    if (cancelledAt === null) return;
    const id = setInterval(() => setTick(Date.now()), 1000);
    return () => clearInterval(id);
  }, [cancelledAt]);
  const run = useMutation({
    mutationFn: async () => {
      onBusy(true);
      setRunning(true);
      setProgress(null);
      setCancelledAt(null);
      setLastCancelAt(null);
      cancel.reset();
      let unlisten: (() => void) | undefined;
      try {
        await beforeTranscribe();
        if (isTauri())
          unlisten = await listen<number>("transcribe-progress", (e) => {
            if (mounted.current) setProgress(e.payload);
          });
        const result = await recordingHistoryApi.transcribe(
          pageId,
          model || undefined,
          language,
          useUi.getState().diarizeEnabled,
        );
        if (mounted.current) onResult(result);
        return result;
      } finally {
        unlisten?.();
        setRunning(false);
        if (mounted.current) onBusy(false);
        await qc.invalidateQueries({ queryKey: recordingHistoryKey });
        await qc.invalidateQueries({
          queryKey: ["transcript-versions", pageId],
        });
        await qc.invalidateQueries({ queryKey: meetingJobsKey });
      }
    },
  });
  const cancel = useMutation({
    mutationFn: () => recordingHistoryApi.cancel(pageId),
    onSuccess: () => {
      const at = Date.now();
      // Keep the first cancel time so the elapsed counter does not restart,
      // and record this attempt so the button re-enables five seconds later.
      setCancelledAt((first) => first ?? at);
      setLastCancelAt(at);
    },
  });
  const selected = versions.data?.find((v) => v.id === version);
  let selectedText = "";
  let versionError: unknown = null;
  if (selected) {
    try {
      selectedText = transcriptText(selected.body_json);
    } catch (error) {
      versionError = error;
    }
  }
  const exporting = useMutation({
    mutationFn: async () => {
      await beforeTranscribe();
      const { invoke } = await import("@/lib/tauri");
      if (isTauri())
        await invoke("export_recording_text", {
          pageId,
          versionId: selected?.id,
        });
      else {
        const text = transcriptText(
          selected?.body_json ?? (await documentsApi.get(pageId)),
        );
        const url = URL.createObjectURL(
          new Blob([text], { type: "text/plain" }),
        );
        const a = document.createElement("a");
        a.href = url;
        a.download = "meeting-transcript.txt";
        document.body.append(a);
        a.click();
        a.remove();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
    },
  });
  const audioExport = useMutation({
    mutationFn: async () => {
      const { invoke } = await import("@/lib/tauri");
      await invoke("export_recording_audio", { pageId });
    },
  });
  if (!meeting) return null;
  const busy = run.isPending || meeting.transcript_state === "transcribing";
  // A saved transcript may have been edited by hand, so overwriting it is a
  // deliberate act rather than a single click.
  const needsConfirm = meeting.transcript_state === "saved";
  const cancelWaiting =
    lastCancelAt !== null && tick - lastCancelAt < CANCEL_RETRY_MS;
  const shown =
    busy ||
    !!run.error ||
    !!meeting.transcript_error ||
    (open ?? meeting.transcript_state !== "saved");
  const summary = busy
    ? `Transcribing${progress === null ? "" : ` · ${progress}%`}`
    : [
        meeting.model_used,
        meeting.audio_available
          ? meeting.audio_temporary
            ? "audio kept until transcribed"
            : "audio kept"
          : "audio not kept",
      ]
        .filter(Boolean)
        .join(" · ");
  return (
    <section
      className="my-6 rounded-xl border border-border bg-bg-subtle"
      aria-label="Recording and transcript"
    >
      <button
        className="flex w-full items-center gap-3 px-5 py-3.5 text-left"
        aria-expanded={shown}
        onClick={() => setOpen(!shown)}
      >
        <AudioLines className="size-4 shrink-0 text-text-faint" aria-hidden />
        <span className="min-w-0 flex-1">
          <h2 className="inline text-sm font-semibold">
            Recording and transcript
          </h2>
          <span className="ml-2 text-xs text-text-faint">{summary}</span>
        </span>
        <ChevronDown
          className={cn(
            "size-4 text-text-faint transition-transform",
            !shown && "-rotate-90",
          )}
          aria-hidden
        />
      </button>
      {shown && (
        <div className="border-t border-border px-5 pb-5 pt-4">
          <p className="text-sm text-text-muted">
            {meeting.audio_available
              ? meeting.audio_temporary
                ? "Temporary audio is being kept because no transcript has been saved yet. It is removed once a transcript is stored."
                : "Audio is saved on this Mac. Re-transcribing also keeps the previous text."
              : "Audio is unavailable. You can still read and export the saved text."}
          </p>
          <div className="mt-4 flex flex-wrap gap-3">
            <label className="text-xs">
              Transcription model
              <select
                aria-label="Transcription model"
                disabled={busy}
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="mt-1 block rounded border border-border bg-surface p-2"
              >
                <option value="">Selected in Settings</option>
                {models.data
                  ?.filter((m) => m.downloaded)
                  .map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
              </select>
            </label>
            <label className="text-xs">
              Spoken language
              <select
                disabled={busy}
                value={language}
                onChange={(e) => setLanguage(e.target.value)}
                className="mt-1 block rounded border border-border bg-surface p-2"
              >
                <option value="en">English</option>
                <option value="auto">Detect automatically</option>
              </select>
            </label>
          </div>
          <p className="mt-2 text-xs text-text-muted">
            For difficult audio, try Whisper Small or Medium after downloading
            it in Settings. Larger models take longer. This transcribes the
            spoken language; it does not translate.
          </p>
          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              disabled={!meeting.audio_available || busy}
              onClick={() =>
                needsConfirm ? setConfirmOverwrite(true) : run.mutate()
              }
            >
              {busy && <Loader2 className="size-4 animate-spin" />}
              {busy
                ? `Transcribing…${progress === null ? "" : ` ${progress}%`}`
                : meeting.transcript_state === "saved"
                  ? "Re-transcribe saved audio"
                  : "Transcribe saved audio"}
            </Button>
            {busy && (
              <Button
                variant="secondary"
                disabled={cancel.isPending || cancelWaiting}
                onClick={() => cancel.mutate()}
              >
                {(cancel.isPending || cancelledAt !== null) && (
                  <Loader2 className="size-4 animate-spin" />
                )}
                {cancelledAt !== null
                  ? `Cancelling…${elapsedLabel(cancelledAt, tick)}`
                  : "Cancel transcription"}
              </Button>
            )}
            <Button
              variant="secondary"
              disabled={exporting.isPending}
              onClick={() => exporting.mutate()}
            >
              {exporting.isPending && (
                <Loader2 className="size-4 animate-spin" />
              )}
              Export text
            </Button>
            <Button
              variant="secondary"
              disabled={
                !meeting.audio_available || audioExport.isPending || !isTauri()
              }
              onClick={() => audioExport.mutate()}
            >
              {audioExport.isPending && (
                <Loader2 className="size-4 animate-spin" />
              )}
              Export audio
            </Button>
          </div>
          {confirmOverwrite && (
            <div
              role="group"
              aria-label="Confirm re-transcription"
              className="mt-4 space-y-2 rounded-lg border border-border bg-surface p-4 text-sm"
            >
              <p>
                Transcribing again replaces the text on this page, including any
                edits you have made. The current text is archived as a version
                in Transcript history, so you can read or export it afterwards.
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  onClick={() => {
                    setConfirmOverwrite(false);
                    run.mutate();
                  }}
                >
                  Replace the text
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setConfirmOverwrite(false)}
                >
                  Keep what is here
                </Button>
              </div>
            </div>
          )}
          {cancelledAt !== null && busy && (
            <p role="status" className="mt-2 text-xs text-text-muted">
              Waiting for the current chunk to finish. Your audio and previous
              text are kept either way.
            </p>
          )}
          {busy && (
            <p role="status" className="mt-2 text-xs">
              Processing continues if you open another page. Audio and previous
              text remain saved.
            </p>
          )}
          {[
            run.error,
            cancel.error,
            exporting.error,
            audioExport.error,
            versions.error,
            versionError,
            models.error,
            history.error,
          ]
            .filter(Boolean)
            .map((e, i) => (
              <p key={i} role="alert" className="mt-2 text-sm text-danger-c">
                {errorMessage(e)}
              </p>
            ))}
          {!run.error && meeting.transcript_error && (
            <p className="mt-2 text-sm text-danger-c">
              {meeting.transcript_error}
            </p>
          )}
          {run.data && (
            <p role="status" className="mt-2 text-sm">
              {run.data.applied
                ? "New transcript saved. Notes and search will rebuild when local AI is enabled."
                : "A new version was saved in history. Your current page was kept because it changed while transcription ran."}
            </p>
          )}
          <details className="mt-5 border-t border-border pt-4">
            <summary className="cursor-pointer text-sm font-medium">
              Transcript history · {versions.data?.length ?? 0} saved versions
            </summary>
            {versions.isPending && (
              <p role="status" className="mt-3 flex gap-2 text-sm">
                <Loader2 className="size-4 animate-spin" />
                Loading transcript history…
              </p>
            )}
            <select
              aria-label="Saved transcript version"
              value={version}
              onChange={(e) => setVersion(e.target.value)}
              className="my-3 w-full rounded border border-border bg-surface p-2 text-sm"
            >
              <option value="">Choose a saved version</option>
              {versions.data?.map((v) => (
                <option key={v.id} value={v.id}>
                  {new Date(v.created_at).toLocaleString("en-AU")} · {v.reason}{" "}
                  {v.model ? `· ${v.model}` : ""}
                </option>
              ))}
            </select>
            {selected && !versionError && (
              <pre className="max-h-80 overflow-y-auto whitespace-pre-wrap rounded-lg bg-surface p-4 font-sans text-sm leading-relaxed">
                {selectedText}
              </pre>
            )}
            <p className="text-xs text-text-muted">
              Export text downloads the selected version, or the current page
              when no version is selected.
            </p>
          </details>
        </div>
      )}
    </section>
  );
}
