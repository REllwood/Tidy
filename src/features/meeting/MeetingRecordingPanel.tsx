import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import {
  documentsApi,
  modelsApi,
  recordingHistoryApi,
  type RetranscriptionResult,
} from "@/lib/api";
import { meetingJobsKey } from "@/lib/localAi";
import { isTauri } from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { recordingHistoryKey } from "@/lib/api";
import { transcriptText } from "./transcriptText";

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
  const history = useQuery({
    queryKey: recordingHistoryKey,
    queryFn: recordingHistoryApi.list,
    refetchInterval: 3000,
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
  });
  const [model, setModel] = useState("");
  const [language, setLanguage] = useState("en");
  const [version, setVersion] = useState("");
  const [progress, setProgress] = useState<number | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const run = useMutation({
    mutationFn: async () => {
      onBusy(true);
      setProgress(null);
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
        );
        if (mounted.current) onResult(result);
        return result;
      } finally {
        unlisten?.();
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
  return (
    <section
      className="my-6 rounded-xl border border-border bg-bg-subtle p-5"
      aria-label="Recording and transcript"
    >
      <h2 className="text-sm font-semibold">Recording &amp; transcript</h2>
      <p className="mt-2 text-sm text-text-muted">
        {meeting.audio_available
          ? "Audio is saved on this Mac. Re-transcribing also keeps the previous text."
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
        For difficult audio, try Whisper Small or Medium after downloading it in
        Settings. Larger models take longer. This transcribes the spoken
        language; it does not translate.
      </p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button
          disabled={!meeting.audio_available || busy}
          onClick={() => run.mutate()}
        >
          {busy && <Loader2 className="size-4 animate-spin" />}
          {busy
            ? `Transcribing…${progress === null ? "" : ` ${progress}%`}`
            : "Re-transcribe saved audio"}
        </Button>
        {busy && (
          <Button
            variant="secondary"
            disabled={cancel.isPending || cancel.isSuccess}
            onClick={() => cancel.mutate()}
          >
            {(cancel.isPending || cancel.isSuccess) && (
              <Loader2 className="size-4 animate-spin" />
            )}
            {cancel.isSuccess ? "Cancelling…" : "Cancel transcription"}
          </Button>
        )}
        <Button
          variant="secondary"
          disabled={exporting.isPending}
          onClick={() => exporting.mutate()}
        >
          {exporting.isPending && <Loader2 className="size-4 animate-spin" />}
          Export text
        </Button>
        <Button
          variant="secondary"
          disabled={
            !meeting.audio_available || audioExport.isPending || !isTauri()
          }
          onClick={() => audioExport.mutate()}
        >
          {audioExport.isPending && <Loader2 className="size-4 animate-spin" />}
          Export audio
        </Button>
      </div>
      {busy && (
        <p role="status" className="mt-2 text-xs">
          Processing continues if you open another page. Audio and previous text
          remain saved.
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
      ]
        .filter(Boolean)
        .map((e, i) => (
          <p key={i} role="alert" className="mt-2 text-sm text-danger-c">
            {errorMessage(e)}
          </p>
        ))}
      {!run.error && meeting.transcript_error && (
        <p className="mt-2 text-sm text-danger-c">{meeting.transcript_error}</p>
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
          Export text downloads the selected version, or the current page when
          no version is selected.
        </p>
      </details>
    </section>
  );
}
