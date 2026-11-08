import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpRight, CalendarDays, Loader2, Search, Sparkles, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useRef, useState } from "react";
import {
  localAi,
  aiStatusKey,
  meetingJobsKey,
  dateRange,
  isUserProgress,
  type AiStatus,
  type MeetingJob,
  type MeetingSource,
} from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { libraryInterval } from "./polling";
import { useUi } from "@/store/ui";
import { AiProgressView } from "@/features/settings/LocalAiSettings";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  clientOptions,
  meetingMatches,
  scopedMeetings,
  searchWords,
  useMeetingQuestion,
  type MeetingMatch,
} from "./meetingLibrary";
import { isTauri } from "@/lib/tauri";

import { MeetingAnswerText } from "./MeetingAnswerText";

const SUGGESTIONS = [
  "What happened last week?",
  "What are the next steps?",
  "What did we decide in the last meeting?",
  "What is still unresolved?",
];

function sourceLabel(source: MeetingSource) {
  const date = new Date(source.started_at).toLocaleDateString("en-AU");
  if (source.kind === "notes") return `Meeting notes · ${date}`;
  if (source.kind === "page") return `Your page · updated ${date}`;
  return source.timestamp ? `${date} · ${source.timestamp}` : date;
}

export function AskMeetings() {
  const { question, from, through, clientId, result, update } =
    useMeetingQuestion();
  const setQuestion = (question: string) => update({ question });
  const setFrom = (from: string) => update({ from });
  const setThrough = (through: string) => update({ through });
  const setPane = useUi((s) => s.setActivePane);
  const qc = useQueryClient();
  // Only tick while the library is actually preparing something; otherwise the
  // page sits still and refreshes through invalidation.
  const [asking, setAsking] = useState(false);
  const status = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
    refetchInterval: (query) =>
      libraryInterval(
        qc.getQueryData<MeetingJob[]>(meetingJobsKey),
        query.state.data?.progress,
        !!query.state.data?.ready,
        asking,
      ),
  });
  const jobs = useQuery({
    queryKey: meetingJobsKey,
    queryFn: localAi.jobs,
    refetchInterval: (query) => {
      const ai = qc.getQueryData<AiStatus>(aiStatusKey);
      return libraryInterval(
        query.state.data as MeetingJob[] | undefined,
        ai?.progress,
        !!ai?.ready,
        asking,
      );
    },
  });
  const clients = clientOptions(jobs.data ?? []);
  let scoped = jobs.data ?? [];
  let rangeError = "";
  try {
    scoped = scopedMeetings(scoped, clientId, from, through);
  } catch (error) {
    scoped = [];
    rangeError = errorMessage(error);
  }
  const scopeName =
    clients.find(([id]) => id === clientId)?.[1] ??
    (clientId ? "Unavailable client" : "All clients");
  // A late reply to an abandoned question must never appear under a new one.
  const requestId = useRef(0);
  const ask = useMutation({
    mutationFn: async () => {
      const id = ++requestId.current;
      setAsking(true);
      const range = dateRange(from, through);
      const answer = await localAi.ask(
        question.trim(),
        range.from,
        range.until,
        clientId || null,
      );
      if (id !== requestId.current) return null;
      return {
        question: question.trim(),
        scope: `${scopeName} · ${!from && !through ? "All dates" : `${from || "Any date"} – ${through || "No end date"}`}`,
        answer,
      };
    },
    onSuccess: (result) => {
      if (result) update({ result });
    },
    onSettled: () => setAsking(false),
  });
  const submit = () => {
    // Drop the previous answer immediately and abandon any request still running.
    update({ result: null });
    requestId.current++;
    if (ask.isPending) void localAi.cancel().catch(() => undefined);
    ask.mutate();
  };
  const retry = useMutation({
    mutationFn: localAi.retry,
    onSuccess: () => qc.invalidateQueries({ queryKey: meetingJobsKey }),
  });
  const cancel = useMutation({ mutationFn: localAi.cancel });
  // Background meeting processing steps aside for a question, so only work
  // the person started (a download, say) holds it up.
  const busy =
    ask.isPending ||
    isUserProgress(status.data?.progress) ||
    !!status.data?.recording_paused;
  const indexed = scoped.filter((j) => j.indexed).length;
  const answerable = indexed > 0 || scoped.some((j) => j.summary);
  const working = scoped.some((j) => ["summarising", "indexing"].includes(j.state));
  const words = searchWords(question);
  const matches = meetingMatches(scoped, words);
  const days = groupByDay(matches);
  const canAsk =
    !busy &&
    !!status.data?.search_ready &&
    answerable &&
    !rangeError &&
    !!question.trim();
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-5 py-10 sm:px-8">
        <p className="page-eyebrow">Meeting library</p>
        <h1 className="mt-2 font-editorial text-[clamp(30px,3.6vw,40px)] leading-tight tracking-[-0.03em]">
          Ask your meetings
        </h1>
        <p className="mt-2 text-sm text-text-muted">
          Decisions, next steps and context from your meetings, with the sources
          for every answer.
          {!isTauri() && " Browser preview: answers are demonstrations."}
        </p>
        {(status.error || jobs.error) && (
          <p role="alert" className="mt-4 text-sm text-danger-c">
            {errorMessage(status.error || jobs.error)}
          </p>
        )}
        {status.data && !status.data.search_ready && (
          <div className="mt-5 flex flex-wrap items-center gap-3 rounded-xl bg-brand-soft px-4 py-3 text-sm">
            <span className="flex-1">
              Turn on local AI and download Meeting search to ask questions on
              this Mac.
            </span>
            <Button size="sm" onClick={() => setPane({ kind: "settings" })}>
              Set up local AI
            </Button>
          </div>
        )}
        <form
          className="mt-6"
          onSubmit={(e) => {
            e.preventDefault();
            if (canAsk) submit();
          }}
        >
          <div className="flex items-start gap-2 rounded-2xl border border-border bg-surface py-2 pl-4 pr-2 shadow-sm transition-colors focus-within:border-brand">
            <Search className="mt-3 size-4 shrink-0 text-text-faint" aria-hidden />
            <label htmlFor="meeting-question" className="sr-only">
              Your question
            </label>
            <textarea
              id="meeting-question"
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              maxLength={1500}
              rows={1}
              placeholder="When is the launch, and who writes the release notes?"
              className="field-sizing-content max-h-40 min-h-10 flex-1 resize-none bg-transparent py-2 text-[15px] leading-6 outline-none placeholder:text-text-faint"
              disabled={ask.isPending}
              onKeyDown={(e) => {
                // Enter asks; Shift+Enter adds a line.
                if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                  e.preventDefault();
                  e.currentTarget.form?.requestSubmit();
                }
              }}
            />
            {ask.isPending && (
              <Button
                type="button"
                variant="secondary"
                className="mt-0.5 rounded-full"
                disabled={cancel.isPending}
                onClick={() => cancel.mutate()}
              >
                {cancel.isPending && <Loader2 className="size-4 animate-spin" />}
                Cancel
              </Button>
            )}
            <Button type="submit" className="mt-0.5 rounded-full" disabled={!canAsk}>
              {ask.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Sparkles className="size-4" />
              )}
              Ask
            </Button>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
            <label className="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface py-1 pl-3 pr-1 text-text-muted">
              <Users className="size-3" aria-hidden />
              <select
                aria-label="Client"
                value={clientId}
                disabled={ask.isPending}
                onChange={(e) => update({ clientId: e.target.value })}
                className="bg-transparent pr-1 text-text outline-none"
              >
                <option value="">All clients</option>
                {clientId && !clients.some(([id]) => id === clientId) && (
                  <option value={clientId}>Unavailable client</option>
                )}
                {clients.map(([id, name]) => (
                  <option key={id} value={id}>
                    {name}
                  </option>
                ))}
              </select>
            </label>
            <Popover>
              <PopoverTrigger
                className={`inline-flex items-center gap-1.5 rounded-full border border-border bg-surface px-3 py-1.5 ${from || through ? "text-text" : "text-text-muted"}`}
              >
                <CalendarDays className="size-3" aria-hidden />
                {from || through ? `${from || "Any date"} to ${through || "now"}` : "Any date"}
              </PopoverTrigger>
              <PopoverContent className="w-72 space-y-3 p-3">
                <label className="block text-xs text-text-muted">
                  From
                  <input
                    type="date"
                    value={from}
                    disabled={ask.isPending}
                    onChange={(e) => setFrom(e.target.value)}
                    className="mt-1 block w-full rounded-md border border-border bg-surface p-2 text-sm"
                  />
                </label>
                <label className="block text-xs text-text-muted">
                  Through
                  <input
                    type="date"
                    value={through}
                    disabled={ask.isPending}
                    onChange={(e) => setThrough(e.target.value)}
                    className="mt-1 block w-full rounded-md border border-border bg-surface p-2 text-sm"
                  />
                </label>
                {(from || through) && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => update({ from: "", through: "" })}
                  >
                    Any date
                  </Button>
                )}
              </PopoverContent>
            </Popover>
            <span className="ml-auto flex items-center gap-1.5 text-text-faint">
              {(working || status.isPending || jobs.isPending) && (
                <Loader2 className="size-3 animate-spin" aria-hidden />
              )}
              {indexed} of {scoped.length} {scoped.length === 1 ? "meeting" : "meetings"} ready
            </span>
          </div>
          {!question.trim() && (
            <div className="mt-4 flex flex-wrap gap-2" aria-label="Suggested questions">
              {SUGGESTIONS.map((suggestion) => (
                <button
                  key={suggestion}
                  type="button"
                  disabled={ask.isPending}
                  onClick={() => setQuestion(suggestion)}
                  className="rounded-full bg-bg-subtle px-3 py-1.5 text-xs text-text-muted hover:text-text disabled:opacity-50"
                >
                  {suggestion}
                </button>
              ))}
            </div>
          )}
        </form>
        {rangeError && (
          <p role="alert" className="mt-3 text-sm text-danger-c">
            {rangeError}
          </p>
        )}
        {status.data?.progress && (
          <div className="mt-5">
            <AiProgressView progress={status.data.progress} />
          </div>
        )}
        {ask.isPending && !status.data?.progress && (
          <p role="status" className="mt-5 flex gap-2 text-sm text-text-muted">
            <Loader2 className="size-4 animate-spin" />
            Preparing your answer…
          </p>
        )}
        {(ask.error || retry.error || cancel.error) && (
          <p role="alert" className="mt-4 text-sm text-danger-c">
            {errorMessage(ask.error || retry.error || cancel.error)}
          </p>
        )}
        {result && !ask.isPending && (
          <section
            className="mt-7 rounded-2xl border border-border bg-surface p-6 shadow-sm"
            aria-label="Meeting answer"
          >
            <div className="flex items-start gap-2.5">
              <Sparkles className="mt-0.5 size-4 shrink-0 text-brand" aria-hidden />
              <div className="min-w-0">
                <p className="text-sm font-semibold">{result.question}</p>
                <p className="mt-0.5 text-xs text-text-faint">{result.scope}</p>
              </div>
            </div>
            {!!result.answer.interpretation?.length && (
              <div className="mt-3 flex flex-wrap gap-1.5">
                {result.answer.interpretation.map((line) => (
                  <span
                    key={line}
                    className="rounded-full bg-bg-subtle px-2.5 py-1 text-xs text-text-muted"
                  >
                    {line}
                  </span>
                ))}
              </div>
            )}
            <div className="mt-4 text-[15px] leading-7">
              <MeetingAnswerText
                text={result.answer.answer}
                sourceCount={
                  result.answer.grounded === false ? 0 : result.answer.sources.length
                }
              />
            </div>
            {result.answer.coverage && (
              <p className="mt-4 text-xs leading-relaxed text-text-faint">
                Read {result.answer.coverage.passages_used} of{" "}
                {result.answer.coverage.total_passages} indexed passages across{" "}
                {result.answer.coverage.meetings_used} of{" "}
                {result.answer.coverage.total_meetings} meetings in this scope.
                {result.answer.coverage.passages_used <
                result.answer.coverage.total_passages
                  ? " This answer covers selected passages, not the entire conversation history."
                  : ""}
              </p>
            )}
            {result.answer.sources.length > 0 && (
              <div className="mt-5 space-y-2 border-t border-border pt-4">
                <h2 className="text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
                  {result.answer.grounded === false ? "Closest passages" : "Sources"}
                </h2>
                {result.answer.sources.map((source, index) => (
                  <div
                    key={source.id}
                    id={`meeting-source-${index + 1}`}
                    className="flex scroll-mt-6 gap-3 rounded-xl bg-bg-subtle p-3"
                  >
                    <span className="grid size-5 shrink-0 place-items-center rounded-full bg-brand-soft text-2xs font-semibold text-brand">
                      {index + 1}
                    </span>
                    <div className="min-w-0">
                      <button
                        className="inline-flex items-center gap-1 text-left text-sm font-medium text-text hover:text-brand"
                        onClick={() =>
                          setPane({
                            kind: "page",
                            pageId: source.page_id,
                            blockId: source.block_id ?? undefined,
                          })
                        }
                      >
                        {source.title}
                        <ArrowUpRight className="size-3.5" />
                      </button>
                      <p className="text-xs text-text-faint">{sourceLabel(source)}</p>
                      <blockquote className="mt-1.5 whitespace-pre-wrap text-sm leading-relaxed text-text-muted">
                        {source.text}
                      </blockquote>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        )}
        <section className="mt-10" aria-label="Meetings">
          <h2 className="text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
            {words.length
              ? `${matches.length} ${matches.length === 1 ? "meeting mentions" : "meetings mention"} this`
              : "Your meetings"}
          </h2>
          {scoped.length === 0 && !jobs.isPending && (
            <p className="mt-4 text-sm text-text-muted">
              No meetings match this client and these dates. Record a meeting or
              widen the filters.
            </p>
          )}
          {scoped.length > 0 && words.length > 0 && matches.length === 0 && (
            <p className="mt-4 text-sm text-text-muted">
              No notes mention those words. Ask the question to search the
              transcripts too.
            </p>
          )}
          <div className="mt-4 space-y-6">
            {days.map(({ day, items }) => (
              <div key={day.toISOString()} className="grid grid-cols-[3rem_1fr] gap-4">
                <div className="pt-3 text-center">
                  <div className="font-editorial text-2xl leading-none">{day.getDate()}</div>
                  <div className="mt-1 text-2xs font-semibold uppercase tracking-[0.14em] text-text-faint">
                    {day.toLocaleDateString("en-AU", { month: "short" })}
                  </div>
                </div>
                <div className="space-y-2">
                  {items.map(({ job, points }) => (
                    <article
                      key={job.page_id}
                      className="rounded-xl border border-border bg-surface p-4 transition-colors hover:border-border-strong"
                    >
                      <div className="flex items-start gap-2">
                        <button
                          className="min-w-0 flex-1 text-left text-sm font-semibold hover:underline"
                          onClick={() => setPane({ kind: "page", pageId: job.page_id })}
                        >
                          {job.title}
                        </button>
                        <JobState job={job} aiReady={!!status.data?.ready} />
                        {job.state === "error" && (
                          <Button
                            variant="secondary"
                            size="sm"
                            disabled={retry.isPending || busy || !status.data?.ready}
                            onClick={() => retry.mutate(job.page_id)}
                          >
                            {retry.isPending && retry.variables === job.page_id && (
                              <Loader2 className="size-3 animate-spin" />
                            )}
                            Retry
                          </Button>
                        )}
                      </div>
                      <p className="mt-0.5 text-xs text-text-faint">
                        {new Date(job.started_at).toLocaleTimeString("en-AU", {
                          hour: "numeric",
                          minute: "2-digit",
                        })}
                        {job.client_name && ` · ${job.client_name}`}
                      </p>
                      {points.map((point, i) => (
                        <p
                          key={i}
                          className={`mt-2 text-sm leading-6 text-text-muted ${words.length ? "" : "line-clamp-2"}`}
                        >
                          <Highlighted text={point} words={words} />
                        </p>
                      ))}
                      {job.error && (
                        <p className="mt-2 text-xs text-danger-c">{job.error}</p>
                      )}
                    </article>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

/** Meetings in day order, newest day first. */
function groupByDay(matches: MeetingMatch[]) {
  const days: { day: Date; items: MeetingMatch[] }[] = [];
  for (const match of matches) {
    const d = new Date(match.job.started_at);
    const day = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    const last = days[days.length - 1];
    if (last && last.day.getTime() === day.getTime()) last.items.push(match);
    else days.push({ day, items: [match] });
  }
  return days;
}

/** Where a meeting is up to, when it is not simply ready. */
function JobState({ job, aiReady }: { job: MeetingJob; aiReady: boolean }) {
  const label =
    job.state === "summarising"
      ? "Writing notes"
      : job.state === "indexing"
        ? "Preparing search"
        : job.state === "queued"
          ? aiReady
            ? "Waiting"
            : "Waiting for local AI"
          : job.state === "error"
            ? "Needs attention"
            : null;
  if (!label) return null;
  return (
    <span
      className={`inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-2xs font-medium ${job.state === "error" ? "bg-[color-mix(in_srgb,var(--danger-c)_14%,transparent)] text-danger-c" : "bg-bg-subtle text-text-muted"}`}
    >
      {["summarising", "indexing"].includes(job.state) && (
        <Loader2 className="size-3 animate-spin" aria-hidden />
      )}
      {label}
    </span>
  );
}

/** Text with the searched words marked. */
function Highlighted({ text, words }: { text: string; words: string[] }) {
  if (!words.length) return <>{text}</>;
  const pattern = new RegExp(
    `(${words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`,
    "giu",
  );
  return (
    <>
      {text.split(pattern).map((part, i) =>
        i % 2 ? (
          <mark key={i} className="rounded bg-brand-soft px-0.5 text-text">
            {part}
          </mark>
        ) : (
          part
        ),
      )}
    </>
  );
}
