import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { localAi, meetingJobsKey } from "@/lib/localAi";
import { useUi } from "@/store/ui";

export function MeetingSummaryPanel({ pageId }: { pageId: string }) {
  const { data } = useQuery({
    queryKey: meetingJobsKey,
    queryFn: localAi.jobs,
    refetchInterval: 2000,
  });
  const setPane = useUi((s) => s.setActivePane);
  const job = data?.find((j) => j.page_id === pageId);
  if (!job) return null;
  const busy = ["summarising", "indexing"].includes(job.state);
  return (
    <section
      className="my-5 rounded-lg border border-border bg-bg-subtle p-4"
      aria-label="Generated meeting summary"
    >
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">Meeting summary</h2>
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
        <p className="mt-2 text-sm text-text-muted">
          Transcript saved. Waiting for local AI to process this meeting.
        </p>
      )}
      {job.error && (
        <p role="alert" className="mt-2 text-sm text-danger-c">
          {job.error} Open the meeting library to retry.
        </p>
      )}
      {job.summary && (
        <div className="mt-3 space-y-3 text-sm leading-relaxed">
          <p className="whitespace-pre-wrap">{job.summary.summary}</p>
          {job.summary.action_items.length > 0 && (
            <div>
              <h3 className="font-medium">Action items</h3>
              <ul className="mt-1 list-disc space-y-1 pl-5">
                {job.summary.action_items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            </div>
          )}
          {job.summary.decisions.length > 0 && (
            <div>
              <h3 className="font-medium">Decisions</h3>
              <ul className="mt-1 list-disc space-y-1 pl-5">
                {job.summary.decisions.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            </div>
          )}
          <p className="text-xs text-text-faint">
            Generated locally. Check names, dates and commitments against the
            original transcript.
          </p>
        </div>
      )}
    </section>
  );
}
