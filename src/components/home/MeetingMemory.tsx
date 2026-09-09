import { useQuery } from "@tanstack/react-query";
import { ArrowUpRight, Loader2, MessageSquare, Mic } from "lucide-react";
import { localAi, aiStatusKey, meetingJobsKey } from "@/lib/localAi";
import { useUi } from "@/store/ui";
import { useMeetingQuestion } from "@/features/meeting/meetingLibrary";

export function MeetingMemory() {
  const setPane = useUi((s) => s.setActivePane);
  const status = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
    refetchInterval: 5000,
  });
  const jobs = useQuery({
    queryKey: meetingJobsKey,
    queryFn: localAi.jobs,
    refetchInterval: 5000,
  });
  const ready = jobs.data?.filter((j) => j.indexed).length ?? 0;
  const active = jobs.data?.some((j) =>
    ["summarising", "indexing"].includes(j.state),
  );
  const loading = status.isPending || jobs.isPending;
  const questions = [
    "What decisions did we make?",
    "What still needs following up?",
    "How have our priorities changed?",
  ];
  return (
    <section
      aria-label="Meeting memory"
      className="overflow-hidden rounded-xl border border-border bg-brand-soft/35"
    >
      <div className="p-6">
        <div className="flex items-center justify-between text-brand">
          <MessageSquare className="size-5" strokeWidth={1.5} />
          <span className="text-[10px] font-semibold uppercase tracking-[0.15em]">
            Meeting memory
          </span>
        </div>
        <h2 className="mt-7 font-editorial text-[30px] leading-[1.15] tracking-tight">
          Good conversations.
          <br />
          Nothing lost.
        </h2>
        <p className="mt-3 text-note leading-relaxed text-text-muted">
          Bring together decisions, ideas and next steps from your client's
          meetings.
        </p>
        <div className="mt-5 border-y border-border py-3">
          {loading ? (
            <p
              role="status"
              className="flex items-center gap-2 text-xs text-text-muted"
            >
              <Loader2 className="size-3 animate-spin" />
              Checking your library…
            </p>
          ) : status.error || jobs.error ? (
            <p role="alert" className="text-xs text-danger-c">
              Couldn't load the meeting library. Open Ask meetings to retry.
            </p>
          ) : (
            <div className="flex items-center justify-between gap-2 text-xs">
              <span className="text-text-muted">
                {ready} {ready === 1 ? "meeting" : "meetings"} ready to ask
              </span>
              {active ? (
                <span className="flex items-center gap-1 text-brand">
                  <Loader2 className="size-3 animate-spin" />
                  Processing
                </span>
              ) : (
                <span className="text-text-faint">On this Mac</span>
              )}
            </div>
          )}
        </div>
        {status.data?.search_ready && ready > 0 ? (
          <>
            <p className="mt-5 text-2xs font-medium text-text-faint">
              A good place to start
            </p>
            <div className="mt-2 divide-y divide-border/70">
              {questions.map((question) => (
                <button
                  key={question}
                  className="group flex w-full items-center justify-between gap-3 py-3 text-left text-note text-text-muted hover:text-brand"
                  onClick={() => {
                    useMeetingQuestion
                      .getState()
                      .update({
                        question,
                        clientId: "",
                        from: "",
                        through: "",
                      });
                    setPane({ kind: "ask" });
                  }}
                >
                  {question}
                  <ArrowUpRight className="size-3.5 shrink-0 transition-transform group-hover:-translate-y-0.5 group-hover:translate-x-0.5" />
                </button>
              ))}
            </div>
          </>
        ) : (
          <button
            onClick={() =>
              setPane({
                kind:
                  !loading && status.data && !status.data.search_ready
                    ? "settings"
                    : "meeting",
              })
            }
            disabled={loading}
            className="mt-5 flex w-full items-center justify-between gap-3 rounded-lg border border-brand/20 bg-surface/70 px-3 py-3 text-left text-note font-medium hover:bg-surface disabled:opacity-50"
          >
            {loading ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Mic className="size-4 text-brand" />
            )}
            <span className="flex-1">
              {!loading && status.data && !status.data.search_ready
                ? "Set up local AI"
                : "Record your first meeting"}
            </span>
            <ArrowUpRight className="size-3.5" />
          </button>
        )}
      </div>
      <button
        onClick={() => setPane({ kind: "ask" })}
        className="flex w-full items-center justify-between border-t border-border bg-surface/40 px-6 py-4 text-note font-medium text-brand hover:bg-surface/80"
      >
        Open meeting library
        <ArrowUpRight className="size-4" />
      </button>
    </section>
  );
}
