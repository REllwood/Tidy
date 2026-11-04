import type { MeetingSummary } from "@/lib/api";
import { shortStamp } from "./playback";

/**
 * The overview, when it says more than a repeat of one point. Notes written
 * by earlier versions used their first point as the overview.
 */
export function notesOverview(summary: MeetingSummary): string | null {
  const overview = summary.summary?.trim();
  if (!overview) return null;
  const points = [
    ...(summary.sections ?? []).flatMap((s) => s.points),
    ...summary.decisions,
    ...summary.action_items,
    ...(summary.open_questions ?? []),
  ];
  return points.some((p) => p.trim() === overview) ? null : overview;
}

/**
 * Structured text is rendered as React content, never executable model HTML.
 * With `onTime`, each point that knows when it was said shows that time as a
 * button that jumps to the moment.
 */
export function MeetingNotes({
  summary,
  onTime,
}: {
  summary: MeetingSummary;
  onTime?: (stamp: string) => void;
}) {
  const overview = notesOverview(summary);
  const groups = [
    ...(summary.sections ?? []).map((section) => ({
      title: section.heading,
      points: section.points,
    })),
    { title: "Decisions", points: summary.decisions },
    { title: "Next steps", points: summary.action_items },
    { title: "Open questions", points: summary.open_questions ?? [] },
    { title: "Marked moments", points: summary.moments ?? [] },
  ].filter((group) => group.points.length);
  return (
    <div className="space-y-6">
      {(overview || (!groups.length && summary.summary)) && (
        <p className="whitespace-pre-wrap text-sm leading-7 text-text">
          {overview ?? summary.summary}
        </p>
      )}
      {groups.map((group, i) => (
        <section key={`${group.title}-${i}`}>
          <h3 className="text-sm font-semibold text-text">{group.title}</h3>
          <ul className="mt-2 list-disc space-y-2 pl-5 text-sm leading-7 text-text-muted marker:text-brand">
            {group.points.map((point, j) => {
              const stamp = summary.times?.[point];
              return (
                <li key={j} className="pl-1">
                  {point}
                  {stamp && onTime && (
                    <button
                      className="ml-1.5 rounded px-1 align-baseline font-mono text-2xs text-brand hover:bg-brand-soft"
                      onClick={() => onTime(stamp)}
                      title="Play from here"
                    >
                      {shortStamp(stamp)}
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
