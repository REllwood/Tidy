import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, ArrowUpRight } from "lucide-react";
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
import { MeetingLibraryProgress } from "./MeetingLibraryProgress";
import {
  clientOptions,
  scopedMeetings,
  useMeetingQuestion,
} from "./meetingLibrary";
import { isTauri } from "@/lib/tauri";

import { MeetingAnswerText } from "./MeetingAnswerText";

const SUGGESTIONS = [
  "What happened last week?",
  "What are the next steps?",
  "What did we decide in the last meeting?",
  "What's still unresolved?",
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
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-4xl px-5 sm:px-8 py-10">
        <p className="page-eyebrow">Meeting library</p>
        <h1 className="page-title">Ask your meetings</h1>
        <p className="page-description">
          Find decisions, next steps and context across your meetings. Ask about
          a date (“last week”), a client or “the last meeting”. Every answer
          shows where it came from.
        </p>
        {!isTauri() && (
          <p className="mt-2 text-xs text-text-faint">
            Browser preview · answers are demonstrations.
          </p>
        )}
        {(status.isPending || jobs.isPending) && (
          <p role="status" className="mt-6 flex gap-2 text-sm">
            <Loader2 className="size-4 animate-spin" />
            Checking your meeting library…
          </p>
        )}
        {(status.error || jobs.error) && (
          <p role="alert" className="mt-4 text-sm text-danger-c">
            {errorMessage(status.error || jobs.error)}
          </p>
        )}
        {status.data && !status.data.search_ready && (
          <div className="mt-6 rounded-lg border border-border bg-surface p-4">
            <p className="text-sm">
              Enable local AI and download Meeting search to ask questions on
              this Mac.
            </p>
            <Button
              className="mt-3"
              size="sm"
              onClick={() => setPane({ kind: "settings" })}
            >
              Set up local AI
            </Button>
          </div>
        )}
        <div className="mt-6 grid gap-3 sm:grid-cols-3">
          <label className="text-xs font-medium text-text-muted sm:col-span-1">
            Client
            <select
              aria-label="Client"
              value={clientId}
              disabled={ask.isPending}
              onChange={(e) => update({ clientId: e.target.value })}
              className="mt-2 block w-full rounded-lg border border-border bg-surface p-2.5 text-sm"
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
          <p className="self-end pb-2 text-xs leading-relaxed text-text-faint sm:col-span-2">
            Choose a client to connect decisions and follow-ups across their
            meetings. Date filters narrow the same library.
          </p>
        </div>
        <MeetingLibraryProgress
          jobs={scoped}
          progress={status.data?.progress ?? null}
          enabled={!!status.data?.ready}
        />
        <form
          className="workspace-panel mt-6 space-y-4 p-5 sm:p-6"
          onSubmit={(e) => {
            e.preventDefault();
            if (
              !busy &&
              status.data?.search_ready &&
              answerable &&
              !rangeError &&
              question.trim()
            )
              submit();
          }}
        >
          <label htmlFor="meeting-question" className="text-sm font-medium">
            Your question
          </label>
          <textarea
            id="meeting-question"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            maxLength={1500}
            rows={3}
            placeholder="What did we agree about the launch date?"
            className="block w-full resize-y rounded-xl border border-border bg-bg p-4 text-sm leading-relaxed outline-none focus:border-brand"
            disabled={ask.isPending}
            onKeyDown={(e) => {
              // Enter asks; Shift+Enter adds a line.
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                e.currentTarget.form?.requestSubmit();
              }
            }}
          />
          {!question.trim() && (
            <div className="flex flex-wrap gap-2" aria-label="Suggested questions">
              {SUGGESTIONS.map((suggestion) => (
                <button
                  key={suggestion}
                  type="button"
                  disabled={ask.isPending}
                  onClick={() => setQuestion(suggestion)}
                  className="rounded-full border border-border bg-surface px-3 py-1 text-xs text-text-muted hover:border-brand hover:text-text disabled:opacity-50"
                >
                  {suggestion}
                </button>
              ))}
            </div>
          )}
          <div className="flex flex-wrap items-end gap-3">
            <label className="text-xs text-text-muted">
              From
              <input
                type="date"
                value={from}
                disabled={ask.isPending}
                onChange={(e) => setFrom(e.target.value)}
                className="mt-1 block rounded-md border border-border bg-surface p-2 text-sm"
              />
            </label>
            <label className="text-xs text-text-muted">
              Through
              <input
                type="date"
                value={through}
                disabled={ask.isPending}
                onChange={(e) => setThrough(e.target.value)}
                className="mt-1 block rounded-md border border-border bg-surface p-2 text-sm"
              />
            </label>
            <Button
              type="submit"
              disabled={
                busy ||
                !status.data?.search_ready ||
                !question.trim() ||
                !answerable ||
                !!rangeError
              }
            >
              {ask.isPending && <Loader2 className="size-4 animate-spin" />}Ask
              meetings
            </Button>
            {ask.isPending && (
              <Button
                type="button"
                variant="secondary"
                disabled={cancel.isPending}
                onClick={() => cancel.mutate()}
              >
                {cancel.isPending && (
                  <Loader2 className="size-4 animate-spin" />
                )}
                Cancel
              </Button>
            )}
          </div>
        </form>
        <p className="mt-3 text-xs text-text-faint">
          {indexed} of {scoped.length} meetings indexed. Answers come from your
          transcripts, saved meeting notes and your own pages. Check the sources
          before relying on them.
        </p>
        {rangeError && (
          <p role="alert" className="mt-3 text-sm text-danger-c">
            {rangeError}
          </p>
        )}
        {status.data?.progress &&
          !scoped.some((j) =>
            ["summarising", "indexing"].includes(j.state),
          ) && (
            <div className="mt-5">
              <AiProgressView progress={status.data.progress} />
            </div>
          )}
        {ask.isPending && !status.data?.progress && (
          <p role="status" className="mt-5 flex gap-2 text-sm">
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
            className="workspace-panel mt-7 p-6"
            aria-label="Meeting answer"
          >
            <p className="mb-3 text-xs text-text-faint">
              {result.question} · {result.scope}
            </p>
            {!!result.answer.interpretation?.length && (
              <ul className="mb-4 space-y-1 rounded-lg bg-bg-subtle px-3 py-2 text-xs text-text-muted">
                {result.answer.interpretation.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            )}
            <MeetingAnswerText
              text={result.answer.answer}
              sourceCount={
                result.answer.grounded === false
                  ? 0
                  : result.answer.sources.length
              }
            />
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
              <div className="mt-5 space-y-3 border-t border-border pt-4">
                <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                  {result.answer.grounded === false
                    ? "Closest passages"
                    : "Sources"}
                </h2>
                {result.answer.sources.map((source, index) => (
                  <div
                    key={source.id}
                    id={`meeting-source-${index + 1}`}
                    className="scroll-mt-6 rounded-lg bg-bg-subtle p-3"
                  >
                    <button
                      className="flex items-center gap-1 text-left text-sm font-medium text-brand"
                      onClick={() =>
                        setPane({
                          kind: "page",
                          pageId: source.page_id,
                          blockId: source.block_id ?? undefined,
                        })
                      }
                    >
                      {index + 1}. {source.title}
                      <ArrowUpRight className="size-3.5" />
                    </button>
                    <p className="mt-1 text-xs text-text-faint">
                      {sourceLabel(source)}
                    </p>
                    <blockquote className="mt-2 whitespace-pre-wrap text-sm leading-relaxed text-text-muted">
                      {source.text}
                    </blockquote>
                  </div>
                ))}
              </div>
            )}
          </section>
        )}
        <section className="mt-10 border-t border-border pt-5">
          <h2 className="text-sm font-semibold">Meeting processing</h2>
          <p className="mt-1 text-xs text-text-faint">
            Summaries and indexing run while Tidy is open. Interrupted work
            resumes next time; failed work can be retried.
          </p>
          {scoped.length === 0 && (
            <p className="mt-4 text-sm text-text-muted">
              No meetings match this client and date range. Record a meeting or
              broaden your filters.
            </p>
          )}
          <div className="mt-3 divide-y divide-border">
            {scoped.map((job) => (
              <div className="flex items-start gap-3 py-3" key={job.page_id}>
                <div className="min-w-0 flex-1">
                  <button
                    className="text-left text-sm font-medium hover:underline"
                    onClick={() =>
                      setPane({ kind: "page", pageId: job.page_id })
                    }
                  >
                    {job.title}
                  </button>
                  <p className="mt-1 text-xs text-text-faint">
                    {job.client_name ?? "No client"} ·{" "}
                    {new Date(job.started_at).toLocaleDateString("en-AU")}
                    {job.indexed &&
                      ` · ${job.passage_count} ${job.passage_count === 1 ? "passage" : "passages"}`}
                  </p>
                  <p className="mt-1 flex items-center gap-1.5 text-xs text-text-faint">
                    {["summarising", "indexing"].includes(job.state) && (
                      <Loader2 className="size-3 animate-spin" />
                    )}
                    {job.state === "queued"
                      ? status.data?.ready
                        ? "Waiting to process"
                        : "Waiting for local AI setup"
                      : job.state === "ready"
                        ? "Summary and search ready"
                        : job.state === "error"
                          ? "Needs attention"
                          : job.state}
                  </p>
                  {job.error && (
                    <p className="mt-1 text-xs text-danger-c">{job.error}</p>
                  )}
                </div>
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
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
