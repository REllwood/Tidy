import type { MeetingSummary } from "@/lib/api";

/** Structured text is rendered as React content, never executable model HTML. */
export function MeetingNotes({ summary }: { summary: MeetingSummary }) {
  const groups = [
    ...(summary.sections ?? []).map((section) => ({
      title: section.heading,
      points: section.points,
    })),
    { title: "Decisions", points: summary.decisions },
    { title: "Next steps", points: summary.action_items },
    { title: "Open questions", points: summary.open_questions ?? [] },
  ].filter((group) => group.points.length);
  return (
    <div className="space-y-6">
      {!summary.sections?.length && summary.summary && (
        <p className="whitespace-pre-wrap text-sm leading-7 text-text-muted">
          {summary.summary}
        </p>
      )}
      {groups.map((group, i) => (
        <section key={`${group.title}-${i}`}>
          <h3 className="text-sm font-semibold text-text">{group.title}</h3>
          <ul className="mt-2 list-disc space-y-2 pl-5 text-sm leading-7 text-text-muted marker:text-brand">
            {group.points.map((point, j) => (
              <li key={j} className="pl-1">
                {point}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
