import { useQuery } from "@tanstack/react-query";
import { CalendarDays, History } from "lucide-react";
import { calendarApi, meetingBriefApi, type MeetingEvent } from "@/lib/api";
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
export function MeetingNow({ client }: { client: string }) {
  const openPage = useUi((s) => s.openPage);
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
  if (!picked && !brief.data) return null;
  const last = brief.data;
  return (
    <div className="mb-5 space-y-3 rounded-lg border border-border bg-bg-subtle px-4 py-3">
      {picked && (
        <div className="flex items-start gap-2.5">
          <CalendarDays className="mt-0.5 size-4 shrink-0 text-brand" />
          <div className="min-w-0">
            <p className="text-sm font-semibold">
              {picked.now ? "Now" : "Next"}: {picked.event.title}
            </p>
            <p className="text-xs text-text-muted">
              {time(picked.event.start_ms)} to {time(picked.event.end_ms)}
              {picked.event.attendees.length > 0 &&
                ` · ${picked.event.attendees.slice(0, 4).join(", ")}${picked.event.attendees.length > 4 ? ` and ${picked.event.attendees.length - 4} more` : ""}`}
            </p>
            {picked.now && (
              <p className="text-xs text-text-faint">
                The recording will be named after this meeting.
              </p>
            )}
          </div>
        </div>
      )}
      {last && (last.points.length > 0 || last.open_actions.length > 0) && (
        <div className="flex items-start gap-2.5">
          <History className="mt-0.5 size-4 shrink-0 text-text-muted" />
          <div className="min-w-0 text-sm">
            <p className="font-semibold">
              Last time
              <button
                className="ml-2 text-xs font-normal text-brand hover:underline"
                onClick={() => openPage(last.meetings[0].page_id)}
              >
                {last.meetings[0].title},{" "}
                {new Date(last.meetings[0].started_at).toLocaleDateString(
                  "en-AU",
                  { day: "numeric", month: "short" },
                )}
              </button>
            </p>
            {last.points.length > 0 && (
              <ul className="mt-1 list-disc space-y-0.5 pl-4 text-text-muted">
                {last.points.map((p, i) => (
                  <li key={i}>{p}</li>
                ))}
              </ul>
            )}
            {last.open_actions.length > 0 && (
              <>
                <p className="mt-2 text-xs font-medium text-text-faint">
                  Still open
                </p>
                <ul className="mt-0.5 list-disc space-y-0.5 pl-4 text-text-muted">
                  {last.open_actions.map((a, i) => (
                    <li key={i}>{a}</li>
                  ))}
                </ul>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
