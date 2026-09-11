import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { recordingHistoryApi, recordingHistoryKey } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";

export function RecordingHistory() {
  const history = useQuery({
    queryKey: recordingHistoryKey,
    queryFn: recordingHistoryApi.list,
    refetchInterval: 3000,
  });
  const open = useUi((s) => s.openPage);
  const [search, setSearch] = useState("");
  const entries =
    history.data?.filter((m) =>
      `${m.title} ${m.client ?? ""}`
        .toLowerCase()
        .includes(search.toLowerCase()),
    ) ?? [];
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
            className="mt-2 mb-6 w-full rounded-lg border border-border bg-surface p-3"
          />
        </label>
        {history.isPending && (
          <p role="status" className="flex items-center gap-2">
            <Loader2 className="size-4 animate-spin" />
            Loading recording history…
          </p>
        )}
        {history.error && (
          <p role="alert" className="text-danger-c">
            {errorMessage(history.error)}
          </p>
        )}
        {!history.isPending && !history.error && !entries.length && (
          <p className="workspace-panel p-8 text-text-muted">
            {search
              ? "No meetings match this search."
              : "Your saved recordings will appear here, including meetings that still need transcription."}
          </p>
        )}
        <div className="divide-y divide-border rounded-xl border border-border bg-surface">
          {entries.map((m) => (
            <div
              key={m.id}
              className="flex flex-wrap items-center justify-between gap-4 p-5"
            >
              <div>
                <button
                  disabled={!m.page_id}
                  onClick={() => m.page_id && open(m.page_id)}
                  className="text-left font-semibold hover:underline"
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
              <div className="text-right text-xs text-text-muted">
                <p className={m.audio_available ? "text-brand" : ""}>
                  {m.audio_available
                    ? "Original audio saved"
                    : "Audio unavailable · text retained"}
                </p>
                <p className="mt-1 flex items-center justify-end gap-1">
                  {m.transcript_state === "transcribing" && (
                    <Loader2 className="size-3 animate-spin" />
                  )}
                  {m.transcript_state === "saved"
                    ? "Transcript saved"
                    : m.transcript_state === "transcribing"
                      ? "Transcribing…"
                      : m.transcript_state === "error"
                        ? "Needs attention"
                        : "Ready to transcribe"}
                </p>
              </div>
            </div>
          ))}
        </div>
        <p className="mt-6 text-xs leading-relaxed text-text-muted">
          Transcripts are saved automatically. Audio is only kept when Keep
          meeting audio is on in Settings. Saved audio uses about 115 MB per
          hour. Without audio, a meeting cannot be re-transcribed.
        </p>
      </div>
    </div>
  );
}
