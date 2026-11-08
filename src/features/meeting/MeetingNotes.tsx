import { useState, type ReactNode } from "react";
import {
  Bookmark,
  CalendarDays,
  Check,
  ChevronDown,
  CircleHelp,
  Diamond,
  Layers,
  ListChecks,
  Play,
  Sparkles,
} from "lucide-react";
import type { MeetingSummary } from "@/lib/api";
import { cn } from "@/lib/utils";
import { lineStamp, shortStamp, stampSeconds } from "./playback";
import { firstTime, clock, nextStepParts } from "./meetingTimeline";

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

function Heading({
  icon: Icon,
  children,
  aside,
}: {
  icon: typeof Sparkles;
  children: ReactNode;
  aside?: ReactNode;
}) {
  return (
    <div className="mb-2 flex items-center gap-2">
      <Icon className="size-3.5 text-brand" aria-hidden />
      <h3 className="text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
        {children}
      </h3>
      {aside && <div className="ml-auto text-xs text-text-faint">{aside}</div>}
    </div>
  );
}

/** When a point was said, as a button that plays the meeting from there. */
function Time({ stamp, onTime }: { stamp?: string; onTime?: (stamp: string) => void }) {
  if (!stamp || !onTime) return null;
  const seconds = stampSeconds(stamp);
  const label = seconds === null ? shortStamp(stamp) : clock(seconds);
  return (
    <button
      className="inline-flex shrink-0 items-center gap-1 self-start rounded-full px-1.5 py-0.5 font-mono text-2xs tabular-nums text-brand hover:bg-brand-soft"
      onClick={() => onTime(stamp)}
      title="Play from here"
      aria-label={`Play from ${label}`}
    >
      <Play className="size-2.5 fill-current" aria-hidden />
      {label}
    </button>
  );
}

function Row({ mark, children, stamp, onTime }: {
  mark: ReactNode;
  children: ReactNode;
  stamp?: string;
  onTime?: (stamp: string) => void;
}) {
  return (
    <li className="flex gap-3 py-1.5">
      <span className="flex h-6 w-3.5 shrink-0 items-center justify-center" aria-hidden>
        {mark}
      </span>
      <span className="min-w-0 flex-1 text-sm leading-6 text-text">{children}</span>
      <Time stamp={stamp} onTime={onTime} />
    </li>
  );
}

function NextStepRow({
  action,
  done,
  stamp,
  onTime,
  onDone,
  colour,
}: {
  action: string;
  done: boolean;
  stamp?: string;
  onTime?: (stamp: string) => void;
  onDone?: (action: string, done: boolean) => void;
  colour?: string;
}) {
  const { owner, task, due } = nextStepParts(action);
  return (
    <li className="flex gap-3 py-2">
      <button
        role="checkbox"
        aria-checked={done}
        aria-label={done ? `Mark as not done: ${action}` : `Mark as done: ${action}`}
        disabled={!onDone}
        onClick={() => onDone?.(action, !done)}
        className={cn(
          "mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border transition-colors",
          done
            ? "border-brand bg-brand text-brand-fg"
            : "border-border-strong hover:border-brand",
        )}
      >
        {done && <Check className="size-3" strokeWidth={3} />}
      </button>
      <div className="min-w-0 flex-1">
        <p
          className={cn(
            "text-sm leading-6",
            done ? "text-text-faint line-through" : "text-text",
          )}
        >
          {task}
        </p>
        {(owner || due || stamp) && (
          <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-muted">
            {owner && (
              <span className="inline-flex items-center gap-1.5">
                <span
                  className="size-1.5 rounded-full"
                  style={{ background: colour ?? "var(--text-faint)" }}
                  aria-hidden
                />
                {owner}
              </span>
            )}
            {due && (
              <span className="inline-flex items-center gap-1">
                <CalendarDays className="size-3" aria-hidden />
                {due}
              </span>
            )}
            <Time stamp={stamp} onTime={onTime} />
          </div>
        )}
      </div>
    </li>
  );
}

function Topic({
  heading,
  points,
  times,
  onTime,
}: {
  heading: string;
  points: string[];
  times?: Record<string, string>;
  onTime?: (stamp: string) => void;
}) {
  const [open, setOpen] = useState(true);
  const start = firstTime(points, times);
  return (
    <section className="rounded-xl border border-border bg-bg-subtle/60">
      <button
        className="flex w-full items-center gap-3 px-4 py-3 text-left"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <h3 className="min-w-0 flex-1 text-sm font-semibold text-text">{heading}</h3>
        {start !== null && (
          <span className="font-mono text-2xs tabular-nums text-text-faint">
            from {clock(start)}
          </span>
        )}
        <ChevronDown
          className={cn("size-4 text-text-faint transition-transform", !open && "-rotate-90")}
          aria-hidden
        />
      </button>
      {open && (
        <ul className="border-t border-border px-4 py-2">
          {points.map((point, i) => (
            <Row
              key={i}
              mark={<span className="size-1 rounded-full bg-text-faint" />}
              stamp={times?.[point]}
              onTime={onTime}
            >
              {point}
            </Row>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * Structured text is rendered as React content, never executable model HTML.
 * The overview comes first, then what was decided and who is doing what,
 * then the topics. With `onTime`, each point that knows when it was said
 * shows that time as a button that plays the moment. With `onDone`, next
 * steps can be ticked off.
 */
export function MeetingNotes({
  summary,
  onTime,
  onDone,
  colours,
}: {
  summary: MeetingSummary;
  onTime?: (stamp: string) => void;
  onDone?: (action: string, done: boolean) => void;
  /** Speaker colours, so an owner's dot matches the map. */
  colours?: Map<string, string>;
}) {
  const overview = notesOverview(summary);
  const times = summary.times;
  const done = new Set(summary.done ?? []);
  const sections = (summary.sections ?? []).filter((s) => s.points.length);
  const questions = summary.open_questions ?? [];
  const moments = summary.moments ?? [];
  const finished = summary.action_items.filter((a) => done.has(a)).length;
  const fallback = !overview && !sections.length && summary.summary?.trim();
  return (
    <div className="space-y-7">
      {(overview || fallback) && (
        <section>
          <Heading icon={Sparkles}>At a glance</Heading>
          <p className="whitespace-pre-wrap text-[15px] leading-7 text-text">
            {overview ?? summary.summary}
          </p>
        </section>
      )}
      {summary.decisions.length > 0 && (
        <section>
          <Heading icon={Diamond} aside={summary.decisions.length}>
            Decisions
          </Heading>
          <ul>
            {summary.decisions.map((decision, i) => (
              <Row
                key={i}
                mark={<span className="size-2 rotate-45 rounded-[2px] bg-brand" />}
                stamp={times?.[decision]}
                onTime={onTime}
              >
                {decision}
              </Row>
            ))}
          </ul>
        </section>
      )}
      {summary.action_items.length > 0 && (
        <section>
          <Heading icon={ListChecks} aside={`${finished} of ${summary.action_items.length} done`}>
            Next steps
          </Heading>
          <ul className="divide-y divide-border">
            {summary.action_items.map((action, i) => (
              <NextStepRow
                key={i}
                action={action}
                done={done.has(action)}
                stamp={times?.[action]}
                onTime={onTime}
                onDone={onDone}
                colour={colours?.get(nextStepParts(action).owner ?? "")}
              />
            ))}
          </ul>
        </section>
      )}
      {questions.length > 0 && (
        <section>
          <Heading icon={CircleHelp}>Open questions</Heading>
          <ul>
            {questions.map((question, i) => (
              <Row
                key={i}
                mark={<span className="size-1.5 rounded-full border border-brand" />}
                stamp={times?.[question]}
                onTime={onTime}
              >
                {question}
              </Row>
            ))}
          </ul>
        </section>
      )}
      {sections.length > 0 && (
        <section>
          <Heading icon={Layers}>Topics</Heading>
          <div className="space-y-2.5">
            {sections.map((section, i) => (
              <Topic
                key={`${section.heading}-${i}`}
                heading={section.heading}
                points={section.points}
                times={times}
                onTime={onTime}
              />
            ))}
          </div>
        </section>
      )}
      {moments.length > 0 && (
        <section>
          <Heading icon={Bookmark}>Marked moments</Heading>
          <ul>
            {moments.map((moment, i) => {
              const stamp = lineStamp(moment) ?? undefined;
              const said = moment.replace(/^\s*\[[^\]]*\]\s*/, "");
              return (
                <Row
                  key={i}
                  mark={<Bookmark className="size-3 text-brand" />}
                  stamp={stamp}
                  onTime={onTime}
                >
                  {said}
                </Row>
              );
            })}
          </ul>
        </section>
      )}
    </div>
  );
}
