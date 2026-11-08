import { useQuery } from "@tanstack/react-query";
import { CalendarDays, History } from "lucide-react";
import { calendarApi, meetingBriefApi, type LastTime, type MeetingEvent } from "@/lib/api";
import { useUi } from "@/store/ui";

const SOON_MS = 10 * 60_000;

/** The meeting a recording starting now is for, or the next one today. */
export function pickEvent(
  events: MeetingEvent[],
  now: number,
): { event: MeetingEvent; now: boolean } | null {
  const current = events
    .filter((e) => e.start_ms - SOON_MS <= now && now < e.end_ms)
    .sort(
      (a, b) =>
        Number(a.attendees.length === 0) - Number(b.attendees.length === 0) ||
        Math.abs(a.start_ms - now) - Math.abs(b.start_ms - now),
    )[0];
  if (current) return { event: current, now: true };
  const next = events.find((e) => e.start_ms > now);
  return next ? { event: next, now: false } : null;
}

function time(ms: number) {
  return new Date(ms).toLocaleTimeString("en-AU", {
    hour: "numeric",
    minute: "2-digit",
  });
}

/**
 * Before recording: the meeting on the calendar, and what earlier meetings
 * like it decided and left open. Without a calendar event, the brief comes
 * from earlier meetings for the client typed in.
 */
export function useMeetingNow(client: string) {
  const status = useQuery({
    queryKey: ["calendar-status"],
    queryFn: calendarApi.status,
  });
  const calendarOn = !!status.data?.enabled && status.data.access === "granted";
  const events = useQuery({
    queryKey: ["calendar-events"],
    queryFn: calendarApi.events,
    enabled: calendarOn,
    refetchInterval: 60_000,
  });
  const picked = pickEvent(events.data ?? [], Date.now());
  const title = picked?.event.title ?? "";
  const brief = useQuery({
    queryKey: ["last-time", title, picked?.event.series ?? null, client.trim()],
    queryFn: () =>
      meetingBriefApi.lastTime(title, picked?.event.series, client.trim()),
    enabled: !!title || !!client.trim(),
    staleTime: 60_000,
  });
  const last = brief.data;
  return {
    calendarOn,
    calendarChecked: !status.isPending,
    picked,
    last: last && (last.points.length > 0 || last.open_actions.length > 0) ? last : null,
  };
}

/** "in 12 min", "now" or "at 6:20 pm" for an event. */
export function startsLabel(event: MeetingEvent, now: number): string {
  const minutes = Math.round((event.start_ms - now) / 60_000);
  if (now >= event.start_ms) return "on now";
  if (minutes <= 60) return `in ${Math.max(1, minutes)} min`;
  return `at ${time(event.start_ms)}`;
}

/** The calendar event a recording is for, written large. */
export function EventHeader({ picked }: { picked: NonNullable<ReturnType<typeof pickEvent>> }) {
  const { event } = picked;
  const people = event.attendees;
  return (
    <div>
      <p className="flex items-center gap-2 text-2xs font-semibold uppercase tracking-[0.14em] text-brand">
        <CalendarDays className="size-3.5" aria-hidden />
        {picked.now ? "Now" : "Next up"} · {startsLabel(event, Date.now())}
      </p>
      <h2 className="mt-2 font-editorial text-[clamp(28px,3.4vw,36px)] leading-tight tracking-[-0.03em]">
        {event.title}
      </h2>
      <p className="mt-1.5 text-sm text-text-muted">
        {time(event.start_ms)} to {time(event.end_ms)}
        {people.length > 0 &&
          ` · ${people.slice(0, 4).join(", ")}${people.length > 4 ? ` and ${people.length - 4} more` : ""}`}
      </p>
      {picked.now && (
        <p className="mt-1 text-xs text-text-faint">
          The recording will be named after this meeting.
        </p>
      )}
    </div>
  );
}

/** What earlier meetings like this one decided and left open. */
export function LastTimeCard({ last }: { last: LastTime }) {
  const openPage = useUi((s) => s.openPage);
  const first = last.meetings[0];
  return (
    <section className="rounded-2xl border border-border bg-bg-subtle/70 p-5">
      <div className="flex items-center gap-2">
        <History className="size-3.5 text-brand" aria-hidden />
        <h3 className="text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
          Last time
        </h3>
        <button
          className="ml-auto text-xs text-brand hover:underline"
          onClick={() => openPage(first.page_id)}
        >
          {first.title},{" "}
          {new Date(first.started_at).toLocaleDateString("en-AU", {
            day: "numeric",
            month: "short",
          })}
        </button>
      </div>
      {last.points.length > 0 && (
        <ul className="mt-3 space-y-1.5 text-sm leading-6 text-text">
          {last.points.map((p, i) => (
            <li key={i} className="flex gap-2.5">
              <span className="mt-2.5 size-1 shrink-0 rounded-full bg-text-faint" aria-hidden />
              {p}
            </li>
          ))}
        </ul>
      )}
      {last.open_actions.length > 0 && (
        <>
          <p className="mt-4 text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
            Still open
          </p>
          <ul className="mt-2 space-y-1.5 text-sm leading-6 text-text">
            {last.open_actions.map((a, i) => (
              <li key={i} className="flex gap-2.5">
                <span className="mt-1.5 size-3.5 shrink-0 rounded-full border border-border-strong" aria-hidden />
                {a}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
