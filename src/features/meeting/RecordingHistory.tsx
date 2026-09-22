import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AudioLines,
  Download,
  Loader2,
  Mic,
  Trash2,
  Wand2,
} from "lucide-react";
import {
  modelsApi,
  recordingHistoryApi,
  recordingHistoryKey,
  type SavedRecording,
} from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { isTauri } from "@/lib/tauri";
import { useUi } from "@/store/ui";
import { Button } from "@/components/ui/button";
import { languageLabels } from "./transcriptionLanguage";
import { recordingHistoryInterval } from "./polling";
import { invalidateMeetingQueries } from "./invalidation";

type Pending = { id: string; action: "transcribe" | "discard" } | null;

/** Everything the row needs to say about a recording, in one place. */
function rowState(meeting: SavedRecording) {
  if (meeting.transcript_state === "transcribing")
    return { label: "Transcribing…", tone: "busy" as const };
  if (meeting.transcript_state === "saved")
    return { label: "Transcript saved", tone: "done" as const };
  if (meeting.audio_available)
    return { label: "Needs transcription", tone: "attention" as const };
  if (meeting.transcript_state === "error")
    return { label: "Needs attention", tone: "error" as const };
  return { label: "Text only", tone: "muted" as const };
}

const toneClass: Record<string, string> = {
  busy: "bg-brand-soft text-brand",
  done: "bg-[color-mix(in_srgb,var(--success)_14%,transparent)] text-success",
  attention:
    "bg-[color-mix(in_srgb,var(--warning)_16%,transparent)] text-warning",
  error: "bg-[color-mix(in_srgb,var(--danger-c)_14%,transparent)] text-danger-c",
  muted: "bg-bg-subtle text-text-muted",
};

export function RecordingHistory() {
  const qc = useQueryClient();
  const history = useQuery({
    queryKey: recordingHistoryKey,
    queryFn: recordingHistoryApi.list,
    refetchInterval: (query) =>
      recordingHistoryInterval(query.state.data as SavedRecording[] | undefined),
  });
  const models = useQuery({
    queryKey: ["models"],
    queryFn: modelsApi.list,
    staleTime: 30_000,
  });
  const open = useUi((s) => s.openPage);
  const [search, setSearch] = useState("");
  const [pending, setPending] = useState<Pending>(null);
  const [model, setModel] = useState("");
  const [language, setLanguage] = useState("en");

  const transcribe = useMutation({
    mutationFn: (pageId: string) =>
      recordingHistoryApi.transcribe(pageId, model || undefined, language),
    onSettled: (_data, _error, pageId) => {
      setPending(null);
      invalidateMeetingQueries(qc, pageId);
    },
  });
  const discard = useMutation({
    mutationFn: (pageId: string) => recordingHistoryApi.discardAudio(pageId),
    onSettled: (_data, _error, pageId) => {
      setPending(null);
      invalidateMeetingQueries(qc, pageId);
    },
  });
  const exportAudio = useMutation({
    mutationFn: async (pageId: string) => {
      const { invoke } = await import("@/lib/tauri");
      await invoke("export_recording_audio", { pageId });
    },
  });

  const entries =
    history.data?.filter((m) =>
      `${m.title} ${m.client ?? ""}`
        .toLowerCase()
        .includes(search.toLowerCase()),
    ) ?? [];
  const actionError = transcribe.error || discard.error || exportAudio.error;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-5xl px-8 py-10">
        <p className="page-eyebrow">Meetings</p>
        <h1 className="page-title">Recording history</h1>
        <p className="page-description mb-8">
          Review your transcripts and earlier versions. Meetings with saved
          audio can be transcribed again.
        </p>
        <label className="block text-sm">
          Find a meeting
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search meeting or client"
            className="mt-2 mb-6 w-full rounded-lg border border-border bg-surface p-3 outline-none focus:border-brand"
          />
        </label>
        {history.isPending && (
          <div
            role="status"
            className="space-y-3 rounded-xl border border-border bg-surface p-5"
          >
            <p className="flex items-center gap-2 text-sm text-text-muted">
              <Loader2 className="size-4 animate-spin" />
              Loading recording history…
            </p>
            {[0, 1, 2].map((i) => (
              <div key={i} className="space-y-2" aria-hidden>
                <div className="h-4 w-48 animate-pulse rounded bg-bg-subtle" />
                <div className="h-3 w-72 animate-pulse rounded bg-bg-subtle" />
              </div>
            ))}
          </div>
        )}
        {history.error && (
          <p
            role="alert"
            className="flex flex-wrap items-center gap-3 rounded-xl border border-border bg-surface p-5 text-sm text-danger-c"
          >
            {errorMessage(history.error)}
            <Button
              size="sm"
              variant="secondary"
              disabled={history.isFetching}
              onClick={() => history.refetch()}
            >
              {history.isFetching && <Loader2 className="size-3 animate-spin" />}
              Try again
            </Button>
          </p>
        )}
        {actionError && (
          <p role="alert" className="mb-4 text-sm text-danger-c">
            {errorMessage(actionError)}
          </p>
        )}
        {!history.isPending && !history.error && !entries.length && (
          <div className="workspace-panel p-10 text-center">
            <Mic className="mx-auto size-8 text-text-faint" strokeWidth={1.5} />
            <p className="mt-3 font-medium">
              {search ? "No meetings match this search" : "No recordings yet"}
            </p>
            <p className="mx-auto mt-1 max-w-md text-sm leading-relaxed text-text-muted">
              {search
                ? "Try a different meeting title or client name."
                : "Your saved recordings will appear here, including meetings that still need transcription."}
            </p>
          </div>
        )}
        {entries.length > 0 && (
          <div className="divide-y divide-border rounded-xl border border-border bg-surface">
            {entries.map((m) => {
              const st = rowState(m);
              const busy =
                m.transcript_state === "transcribing" ||
                (transcribe.isPending && transcribe.variables === m.page_id);
              const discarding =
                discard.isPending && discard.variables === m.page_id;
              return (
                <div key={m.id} className="p-5">
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div className="min-w-0">
                      <button
                        disabled={!m.page_id}
                        onClick={() => m.page_id && open(m.page_id)}
                        className="text-left font-semibold hover:underline disabled:no-underline disabled:opacity-70"
                      >
                        {m.title}
                      </button>
                      <p className="mt-1 text-xs text-text-muted">
                        {m.client ?? "No client"} ·{" "}
                        {new Date(m.started_at).toLocaleString("en-AU")} ·{" "}
                        {Math.ceil(m.duration_ms / 60000)} min
                      </p>
                      {m.transcript_error && (
                        <p className="mt-2 max-w-xl text-sm text-danger-c">
                          {m.transcript_error}
                        </p>
                      )}
                    </div>
                    <div className="flex shrink-0 flex-col items-end gap-1.5 text-xs">
                      <span
                        className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 font-medium ${toneClass[st.tone]}`}
                      >
                        {st.tone === "busy" && (
                          <Loader2 className="size-3 animate-spin" />
                        )}
                        {st.label}
                      </span>
                      <span className="flex items-center gap-1 text-text-muted">
                        <AudioLines className="size-3" />
                        {m.audio_available
                          ? m.audio_temporary
                            ? "Temporary audio kept"
                            : "Original audio saved"
                          : "Audio unavailable · text retained"}
                      </span>
                    </div>
                  </div>
                  {m.audio_temporary && m.transcript_state !== "saved" && (
                    <p className="mt-3 rounded-lg bg-bg-subtle px-3 py-2 text-xs leading-relaxed text-text-muted">
                      This meeting has no saved transcript yet, so its audio is
                      being kept even though Keep meeting audio is off. Transcribe
                      it to finish, or discard the audio to free the space.
                    </p>
                  )}
                  {m.page_id && (m.audio_available || isTauri()) && (
                    <div className="mt-3 flex flex-wrap gap-2">
                      {m.audio_available && (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={busy || discarding}
                          onClick={() =>
                            setPending(
                              pending?.id === m.id &&
                                pending.action === "transcribe"
                                ? null
                                : { id: m.id, action: "transcribe" },
                            )
                          }
                        >
                          {busy ? (
                            <Loader2 className="size-3.5 animate-spin" />
                          ) : (
                            <Wand2 className="size-3.5" />
                          )}
                          {busy
                            ? "Transcribing…"
                            : m.transcript_state === "saved"
                              ? "Transcribe again"
                              : "Transcribe"}
                        </Button>
                      )}
                      {m.audio_available && (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={!isTauri() || exportAudio.isPending}
                          onClick={() =>
                            m.page_id && exportAudio.mutate(m.page_id)
                          }
                        >
                          {exportAudio.isPending &&
                            exportAudio.variables === m.page_id && (
                              <Loader2 className="size-3.5 animate-spin" />
                            )}
                          <Download className="size-3.5" />
                          Export audio
                        </Button>
                      )}
                      {m.audio_available && (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={busy || discarding}
                          onClick={() =>
                            setPending(
                              pending?.id === m.id && pending.action === "discard"
                                ? null
                                : { id: m.id, action: "discard" },
                            )
                          }
                        >
                          <Trash2 className="size-3.5" />
                          Discard audio…
                        </Button>
                      )}
                    </div>
                  )}
                  {pending?.id === m.id && pending.action === "transcribe" && (
                    <div
                      role="group"
                      aria-label={`Transcribe ${m.title}`}
                      className="mt-3 space-y-3 rounded-lg border border-border bg-bg-subtle p-4"
                    >
                      <div className="flex flex-wrap gap-3">
                        <label className="text-xs text-text-muted">
                          Transcription model
                          <select
                            aria-label="Transcription model"
                            value={model}
                            disabled={busy}
                            onChange={(e) => setModel(e.target.value)}
                            className="mt-1 block rounded-md border border-border bg-surface p-2 text-sm text-text"
                          >
                            <option value="">Selected in Settings</option>
                            {models.data
                              ?.filter((x) => x.downloaded)
                              .map((x) => (
                                <option key={x.id} value={x.id}>
                                  {x.name}
                                </option>
                              ))}
                          </select>
                        </label>
                        <label className="text-xs text-text-muted">
                          Spoken language
                          <select
                            aria-label="Spoken language"
                            value={language}
                            disabled={busy}
                            onChange={(e) => setLanguage(e.target.value)}
                            className="mt-1 block rounded-md border border-border bg-surface p-2 text-sm text-text"
                          >
                            <option value="en">{languageLabels.en}</option>
                            <option value="auto">{languageLabels.auto}</option>
                          </select>
                        </label>
                      </div>
                      {models.error && (
                        <p role="alert" className="text-xs text-danger-c">
                          {errorMessage(models.error)}
                        </p>
                      )}
                      <div className="flex flex-wrap gap-2">
                        <Button
                          size="sm"
                          disabled={busy}
                          onClick={() =>
                            m.page_id && transcribe.mutate(m.page_id)
                          }
                        >
                          {busy && <Loader2 className="size-3.5 animate-spin" />}
                          Start transcription
                        </Button>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => setPending(null)}
                        >
                          Not now
                        </Button>
                      </div>
                    </div>
                  )}
                  {pending?.id === m.id && pending.action === "discard" && (
                    <div
                      role="group"
                      aria-label={`Discard audio for ${m.title}`}
                      className="mt-3 space-y-2 rounded-lg border border-border bg-bg-subtle p-4 text-sm"
                    >
                      <p>
                        Discard the audio for {m.title}? Every saved transcript
                        and version stays, but this meeting can no longer be
                        transcribed again.
                      </p>
                      <div className="flex flex-wrap gap-2">
                        <Button
                          size="sm"
                          variant="destructive"
                          disabled={discarding}
                          onClick={() => m.page_id && discard.mutate(m.page_id)}
                        >
                          {discarding && (
                            <Loader2 className="size-3.5 animate-spin" />
                          )}
                          Discard audio
                        </Button>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => setPending(null)}
                        >
                          Keep audio
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
        <p className="mt-6 text-xs leading-relaxed text-text-muted">
          Transcripts are saved automatically. Audio is only kept when Keep
          meeting audio is on in Settings, or while a meeting still needs
          transcribing. Saved audio uses about 115 MB per hour. Without audio, a
          meeting cannot be transcribed again.
        </p>
      </div>
    </div>
  );
}
