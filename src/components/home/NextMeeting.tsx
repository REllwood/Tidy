import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CalendarDays, History, Loader2 } from "lucide-react";
import { calendarApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { useUi } from "@/store/ui";
import { startsLabel, useMeetingNow } from "@/features/meeting/MeetingNow";
import { calendarStatusKey } from "@/features/settings/CalendarSettings";

function time(ms: number) {
  return new Date(ms).toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit" });
}

/**
 * The next meeting on the calendar, a step away from recording it, with
 * what the last meeting like it decided and left open.
 */
export function NextMeeting() {
  const setPane = useUi((s) => s.setActivePane);
  const openPage = useUi((s) => s.openPage);
  const qc = useQueryClient();
  const { calendarOn, calendarChecked, picked, last } = useMeetingNow("");
  const connect = useMutation({
    mutationFn: calendarApi.connect,
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: calendarStatusKey });
      void qc.invalidateQueries({ queryKey: ["calendar-events"] });
    },
  });
  if (!calendarChecked) return null;
  if (!calendarOn)
    return (
      <section
        aria-label="Next meeting"
        className="rounded-2xl border border-dashed border-border-strong p-5"
      >
        <p className="flex items-center gap-2 text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
          <CalendarDays className="size-3.5" aria-hidden />
          Next meeting
        </p>
        <p className="mt-2 text-sm leading-relaxed text-text-muted">
          Use your calendar to see your next meeting here, with what last time
          decided and left open.
        </p>
        <Button
          size="sm"
          variant="outline"
          className="mt-3 rounded-full"
          disabled={connect.isPending}
          onClick={() => connect.mutate()}
        >
          {connect.isPending && <Loader2 className="size-3.5 animate-spin" />}
          Use my calendar
        </Button>
      </section>
    );
  return (
    <section
      aria-label="Next meeting"
      className="rounded-2xl border border-border bg-surface p-5 shadow-sm"
    >
      <p className="flex items-center gap-2 text-2xs font-semibold uppercase tracking-[0.14em] text-brand">
        <CalendarDays className="size-3.5" aria-hidden />
        {picked ? `${picked.now ? "Now" : "Next up"} · ${startsLabel(picked.event, Date.now())}` : "Next meeting"}
      </p>
      {picked ? (
        <>
          <h2 className="mt-2 font-editorial text-2xl leading-tight tracking-[-0.02em]">
            {picked.event.title}
          </h2>
          <p className="mt-1 text-sm text-text-muted">
            {time(picked.event.start_ms)} to {time(picked.event.end_ms)}
            {picked.event.attendees.length > 0 &&
              ` · ${picked.event.attendees.slice(0, 3).join(", ")}${picked.event.attendees.length > 3 ? ` and ${picked.event.attendees.length - 3} more` : ""}`}
          </p>
          <Button
            className="mt-4 rounded-full"
            onClick={() => setPane({ kind: "meeting" })}
          >
            <span className="size-2.5 rounded-full bg-rec" aria-hidden />
            Record
          </Button>
          {last && (
            <div className="mt-5 border-t border-border pt-4">
              <div className="flex items-center gap-2">
                <History className="size-3.5 text-text-faint" aria-hidden />
                <p className="text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
                  Last time
                </p>
                <button
                  className="ml-auto text-xs text-brand hover:underline"
                  onClick={() => openPage(last.meetings[0].page_id)}
                >
                  Open notes
                </button>
              </div>
              <ul className="mt-2 space-y-1 text-sm leading-6 text-text">
                {last.points.slice(0, 2).map((p, i) => (
                  <li key={i} className="flex gap-2.5">
                    <span className="mt-2.5 size-1 shrink-0 rounded-full bg-text-faint" aria-hidden />
                    {p}
                  </li>
                ))}
              </ul>
              {last.open_actions.length > 0 && (
                <p className="mt-2 text-xs text-text-muted">
                  {last.open_actions.length} {last.open_actions.length === 1 ? "next step" : "next steps"} still open
                </p>
              )}
            </div>
          )}
        </>
      ) : (
        <p className="mt-2 text-sm text-text-muted">
          Nothing else on your calendar today.
        </p>
      )}
    </section>
  );
}
