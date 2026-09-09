import { Loader2 } from "lucide-react";
import type { AiProgress, MeetingJob } from "@/lib/localAi";
import { AiProgressView } from "@/features/settings/LocalAiSettings";

export function MeetingLibraryProgress({
  jobs,
  progress,
  enabled,
}: {
  jobs: MeetingJob[];
  progress: AiProgress | null;
  enabled: boolean;
}) {
  const ready = jobs.filter((j) => j.indexed).length;
  const waiting = jobs.filter((j) => j.state === "queued").length;
  const errors = jobs.filter((j) => j.state === "error").length;
  const active = jobs.find((j) =>
    ["summarising", "indexing"].includes(j.state),
  );
  const passages = jobs.reduce((n, j) => n + j.passage_count, 0);
  const percent = jobs.length ? Math.round((ready / jobs.length) * 100) : 0;
  return (
    <section
      aria-label="Library readiness"
      className="mt-7 overflow-hidden rounded-xl border border-border bg-surface"
    >
      <div className="flex flex-wrap items-center justify-between gap-4 p-5">
        <div>
          <p className="text-xs font-medium uppercase tracking-wider text-text-faint">
            Search readiness
          </p>
          <p className="mt-2 text-lg font-semibold">
            {ready}{" "}
            <span className="font-normal text-text-muted">
              of {jobs.length} {jobs.length === 1 ? "meeting" : "meetings"}{" "}
              ready
            </span>
          </p>
          <p className="mt-1 text-xs text-text-muted">
            {passages.toLocaleString("en-AU")} searchable{" "}
            {passages === 1 ? "passage" : "passages"} · kept on this Mac
          </p>
        </div>
        <span className="font-mono text-3xl font-medium tabular-nums text-brand">
          {percent}%
        </span>
      </div>
      <div className="px-5">
        <div
          role="progressbar"
          aria-label="Meetings ready for search"
          aria-valuemin={0}
          aria-valuemax={Math.max(jobs.length, 1)}
          aria-valuenow={ready}
          aria-valuetext={`${ready} of ${jobs.length} meetings ready`}
          className="h-1.5 overflow-hidden rounded-full bg-bg-subtle"
        >
          <div
            className="h-full rounded-full bg-brand transition-[width] duration-500 motion-reduce:transition-none"
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>
      <div className="grid grid-cols-3 gap-3 px-5 py-4 text-xs">
        <div>
          <span className="block text-lg font-semibold tabular-nums">
            {waiting}
          </span>
          <span className="text-text-faint">Queued</span>
        </div>
        <div>
          <span className="flex items-center gap-2 text-lg font-semibold tabular-nums">
            {active ? 1 : 0}
            {active && (
              <Loader2 aria-hidden className="size-3 animate-spin text-brand" />
            )}
          </span>
          <span className="text-text-faint">Processing</span>
        </div>
        <div>
          <span
            className={`block text-lg font-semibold tabular-nums ${errors ? "text-danger-c" : ""}`}
          >
            {errors}
          </span>
          <span className="text-text-faint">Need attention</span>
        </div>
      </div>
      <div className="border-t border-border bg-bg-subtle px-5 py-4">
        {active ? (
          <>
            <p className="mb-2 truncate text-sm font-medium">{active.title}</p>
            <div className="mb-3 flex gap-4 text-xs text-text-muted">
              <span>1. Transcript saved</span>
              <span
                className={active.state === "summarising" ? "text-brand" : ""}
              >
                2. Summary
              </span>
              <span className={active.state === "indexing" ? "text-brand" : ""}>
                3. Search index
              </span>
            </div>
            {progress ? (
              <AiProgressView progress={progress} />
            ) : (
              <p role="status" className="flex items-center gap-2 text-xs">
                <Loader2 className="size-3 animate-spin" />
                Preparing local model…
              </p>
            )}
          </>
        ) : (
          <p className="text-xs leading-relaxed text-text-muted">
            {!jobs.length
              ? "Record a meeting and choose a client to start building their meeting history."
              : !enabled
                ? "Enable local AI in Settings to process saved transcripts."
                : waiting
                  ? "Queued meetings will process automatically while Tidy is open. Recording pauses AI processing."
                  : errors
                    ? "Some meetings need attention. Review the details below and retry when ready."
                    : "Your indexed meetings are ready to ask about. New recordings will be added automatically."}
          </p>
        )}
      </div>
    </section>
  );
}
