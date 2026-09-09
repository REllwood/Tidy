import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { localAi, meetingJobsKey, aiStatusKey } from "@/lib/localAi";
import { useUi } from "@/store/ui";

import { MeetingNotes } from "./MeetingNotes";
import { Button } from "@/components/ui/button";
import { errorMessage } from "@/lib/errors";

export function MeetingSummaryPanel({ pageId }: { pageId: string }) {
  const { data } = useQuery({
    queryKey: meetingJobsKey,
    queryFn: localAi.jobs,
    refetchInterval: 2000,
  });
  const qc = useQueryClient();
  const status = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
    refetchInterval: 2000,
  });
  const regenerate = useMutation({
    mutationFn: () => localAi.retry(pageId),
    onSuccess: () => qc.invalidateQueries({ queryKey: meetingJobsKey }),
  });
  const setPane = useUi((s) => s.setActivePane);
  const job = data?.find((j) => j.page_id === pageId);
  if (!job) return null;
  const busy = ["summarising", "indexing"].includes(job.state);
  return (
    <section
      className="my-6 rounded-xl border border-border bg-surface p-6 sm:p-8"
      aria-label="Generated meeting summary"
    >
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">Meeting notes</h2>
        <button
          className="text-xs text-brand hover:underline"
          onClick={() => setPane({ kind: "ask" })}
        >
          Meeting library
        </button>
      </div>
      {busy && (
        <p
          role="status"
          className="mt-2 flex items-center gap-2 text-xs text-text-muted"
        >
          <Loader2 className="size-3 animate-spin" />
          {job.state === "summarising"
            ? "Summarising locally…"
            : "Preparing meeting search…"}
        </p>
      )}
      {job.state === "queued" && (
        <p
          role="status"
          className="mt-2 flex items-center gap-2 text-sm text-text-muted"
        >
          <Loader2 className="size-3 animate-spin" /> Transcript saved. Waiting
          for local AI to process this meeting.
        </p>
      )}
      {job.error && (
        <p role="alert" className="mt-2 text-sm text-danger-c">
          {job.error} Open the meeting library to retry.
        </p>
      )}
      {regenerate.error && (
        <p role="alert" className="mt-3 text-sm text-danger-c">
          {errorMessage(regenerate.error)}
        </p>
      )}
      {job.summary && (
        <div className="mt-3 space-y-3 text-sm leading-relaxed">
          <MeetingNotes summary={job.summary} />
          <p className="text-xs text-text-faint">
            Generated locally. Check names, dates and commitments against the
            original transcript.
          </p>
        </div>
      )}
      <div className="mt-5 border-t border-border pt-4">
        <Button
          variant="secondary"
          size="sm"
          disabled={
            regenerate.isPending ||
            busy ||
            job.state === "queued" ||
            !status.data?.ready ||
            !!status.data.progress ||
            status.data.recording_paused
          }
          onClick={() => regenerate.mutate()}
        >
          {regenerate.isPending && <Loader2 className="size-3 animate-spin" />}
          Regenerate notes
        </Button>
      </div>
    </section>
  );
}
