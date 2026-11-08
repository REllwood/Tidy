import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDown, ChevronDown, Loader2, MoreHorizontal, Users } from "lucide-react";
import { toast } from "sonner";
import {
  localAi,
  meetingJobsKey,
  aiStatusKey,
  isUserProgress,
  type MeetingJob,
} from "@/lib/localAi";
import {
  databasesApi,
  documentsApi,
  meetingBriefApi,
  meetingNotesApi,
  recordingHistoryApi,
  recordingHistoryKey,
  type MeetingSummary,
  type SavedRecording,
} from "@/lib/api";
import { flushPendingEdits } from "@/lib/pendingEdits";
import { announceDocumentChange } from "@/hooks/useDocument";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SpeakerNamesDialog } from "./SpeakerNames";
import { MeetingMap, type MapMark } from "./MeetingMap";
import { lineStamp, revealLine, revealTranscript, stampSeconds, usePlayback } from "./playback";
import { firstTime, speakerColours, talkShare, transcriptTurns } from "./meetingTimeline";
import { useUi } from "@/store/ui";

import { MeetingNotes } from "./MeetingNotes";
import { Button } from "@/components/ui/button";
import { errorMessage } from "@/lib/errors";
import { isMeetingPage, summaryPanelInterval } from "./polling";

/** "Sat 8 Nov · 2:20 pm · 32 min · Acme" */
function meetingLine(job: MeetingJob, recording?: SavedRecording): string {
  const when = new Date(recording?.started_at ?? job.started_at);
  const parts = [
    when.toLocaleDateString("en-AU", { weekday: "short", day: "numeric", month: "short" }),
    when.toLocaleTimeString("en-AU", { hour: "numeric", minute: "2-digit" }),
  ];
  if (recording?.duration_ms) parts.push(`${Math.max(1, Math.round(recording.duration_ms / 60_000))} min`);
  if (job.client_name) parts.push(job.client_name);
  return parts.join(" · ");
}

/** Where topics start, the decisions and the marked moments, for the map. */
function mapMarks(summary: MeetingSummary): MapMark[] {
  const at = (point: string) => {
    const stamp = summary.times?.[point];
    return stamp ? stampSeconds(stamp) : null;
  };
  const marks: MapMark[] = [];
  for (const section of summary.sections ?? []) {
    const start = firstTime(section.points, summary.times);
    if (start !== null) marks.push({ at: start, kind: "topic", label: section.heading });
  }
  for (const decision of summary.decisions) {
    const start = at(decision);
    if (start !== null) marks.push({ at: start, kind: "decision", label: decision });
  }
  for (const moment of summary.moments ?? []) {
    const stamp = lineStamp(moment);
    const start = stamp ? stampSeconds(stamp) : null;
    if (start !== null) marks.push({ at: start, kind: "moment", label: moment.replace(/^\s*\[[^\]]*\]\s*/, "") });
  }
  return marks;
}

export function MeetingSummaryPanel({ pageId }: { pageId: string }) {
  const qc = useQueryClient();
  // Rendered on every document page: decide cheaply whether this is a meeting
  // at all, and only then look at the AI queue.
  const history = useQuery({
    queryKey: recordingHistoryKey,
    queryFn: recordingHistoryApi.list,
    staleTime: 30_000,
  });
  const isMeeting = isMeetingPage(
    history.data as SavedRecording[] | undefined,
    pageId,
  );
  const status = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
    enabled: isMeeting,
    staleTime: 30_000,
  });
  const aiReady = !!status.data?.ready;
  const { data } = useQuery({
    queryKey: meetingJobsKey,
    queryFn: localAi.jobs,
    enabled: isMeeting,
    refetchInterval: (query) =>
      summaryPanelInterval(
        query.state.data as MeetingJob[] | undefined,
        pageId,
        aiReady,
      ),
  });
  const regenerate = useMutation({
    mutationFn: () => localAi.retry(pageId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: meetingJobsKey });
      void qc.invalidateQueries({ queryKey: aiStatusKey });
    },
  });
  const setPane = useUi((s) => s.setActivePane);
  const [naming, setNaming] = useState(false);
  const addToPage = useMutation({
    mutationFn: async () => {
      // Save any edit still in the editor before the page is rewritten.
      await flushPendingEdits();
      return meetingNotesApi.addToPage(pageId);
    },
    onSuccess: (replaced) => {
      announceDocumentChange(qc, pageId);
      toast.success(
        replaced
          ? "Updated the notes in the page"
          : "Added the notes to the page",
      );
    },
    onError: (error) =>
      toast.error(`Couldn't add the notes. ${errorMessage(error)}`),
  });
  const databases = useQuery({
    queryKey: ["databases"],
    queryFn: () => databasesApi.list(),
    enabled: isMeeting,
  });
  const toTasks = useMutation({
    mutationFn: (db: { id: string; title: string }) =>
      meetingNotesApi
        .nextStepsToTasks(pageId, db.id)
        .then((result) => ({ ...result, title: db.title })),
    onSuccess: ({ added, skipped, title }) => {
      void qc.invalidateQueries({ queryKey: ["databases"] });
      void qc.invalidateQueries({ queryKey: ["database-by-id"] });
      toast.success(
        added
          ? `Added ${added} next ${added === 1 ? "step" : "steps"} to ${title}${skipped ? ` (${skipped} already there)` : ""}`
          : `These next steps are already in ${title}`,
      );
    },
    onError: (error) =>
      toast.error(`Couldn't add the next steps. ${errorMessage(error)}`),
  });
  const layouts = useQuery({
    queryKey: ["note-layouts"],
    queryFn: meetingBriefApi.layouts,
    enabled: isMeeting,
    staleTime: Infinity,
  });
  const relayout = useMutation({
    mutationFn: (template: string) => meetingBriefApi.setLayout(pageId, template),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: meetingJobsKey });
      void qc.invalidateQueries({ queryKey: aiStatusKey });
    },
    onError: (error) =>
      toast.error(`Couldn't rewrite the notes. ${errorMessage(error)}`),
  });
  // Ticking a next step shows at once and is saved with the notes.
  const tick = useMutation({
    mutationFn: ({ action, done }: { action: string; done: boolean }) =>
      meetingNotesApi.setDone(pageId, action, done),
    onMutate: ({ action, done }) => {
      qc.setQueryData<MeetingJob[]>(meetingJobsKey, (jobs) =>
        jobs?.map((j) =>
          j.page_id === pageId && j.summary
            ? {
                ...j,
                summary: {
                  ...j.summary,
                  done: done
                    ? [...(j.summary.done ?? []).filter((d) => d !== action), action]
                    : (j.summary.done ?? []).filter((d) => d !== action),
                },
              }
            : j,
        ),
      );
    },
    onError: (error) => {
      void qc.invalidateQueries({ queryKey: meetingJobsKey });
      toast.error(`Couldn't save that. ${errorMessage(error)}`);
    },
  });
  const play = usePlayback((s) => s.play);
  // Names the conversation points to that have not been given yet.
  const speakers = useQuery({
    queryKey: ["meeting-speakers", pageId],
    queryFn: () => meetingNotesApi.speakers(pageId),
    enabled: isMeeting,
    staleTime: 60_000,
  });
  const suggested = (speakers.data ?? []).filter((s) => s.suggestion);
  const recording = (history.data as SavedRecording[] | undefined)?.find(
    (r) => r.page_id === pageId,
  );
  const job = data?.find((j) => j.page_id === pageId);
  // Who spoke when, read from the transcript in the page. It is read again
  // when the transcript or the notes change.
  const transcript = useQuery({
    queryKey: ["meeting-transcript", pageId, recording?.transcript_state, job?.state, speakers.dataUpdatedAt],
    queryFn: () => documentsApi.get(pageId),
    enabled: isMeeting && !!job,
    staleTime: 30_000,
  });
  const audioAvailable = !!recording?.audio_available;
  if (!job) return null;
  const layout = layouts.data?.find((l) => l.id === job.template);
  const duration = recording?.duration_ms ? recording.duration_ms / 1000 : undefined;
  const turns = transcript.data ? transcriptTurns(transcript.data, duration) : [];
  const length = duration ?? turns[turns.length - 1]?.end ?? 0;
  const talkers = talkShare(turns);
  const colours = speakerColours(turns.flatMap((t) => (t.speaker ? [t.speaker] : [])));
  const jump = (stamp: string) => {
    revealLine(stamp);
    const seconds = stampSeconds(stamp);
    if (audioAvailable && seconds !== null) play(pageId, seconds);
  };
  const busy = ["summarising", "indexing"].includes(job.state);
  const canRewrite = !busy && job.state !== "queued" && !!status.data?.ready;
  return (
    <section
      className="my-6 rounded-2xl border border-border bg-surface p-5 shadow-sm sm:p-7"
      aria-label="Generated meeting summary"
    >
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="page-eyebrow">
            Meeting notes
            {layout && layout.id !== "general" && <> · {layout.name}</>}
          </p>
          <p className="mt-1.5 text-sm text-text-muted">{meetingLine(job, recording)}</p>
        </div>
        <Button variant="ghost" size="sm" onClick={() => setNaming(true)}>
          <Users className="size-3.5" />
          Name speakers
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={<Button variant="ghost" size="icon" aria-label="More for these notes" />}
          >
            <MoreHorizontal className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-60">
            <DropdownMenuItem
              disabled={
                regenerate.isPending ||
                !canRewrite ||
                // Background work can keep running; only a setup or download
                // the person started here blocks a new request.
                isUserProgress(status.data?.progress) ||
                !!status.data?.recording_paused
              }
              onClick={() => regenerate.mutate()}
            >
              Regenerate notes
            </DropdownMenuItem>
            {!!layouts.data?.length && (
              <DropdownMenuSub>
                <DropdownMenuSubTrigger disabled={relayout.isPending || !canRewrite}>
                  Rewrite notes as
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="w-64">
                  {layouts.data.map((l) => (
                    <DropdownMenuItem
                      key={l.id}
                      disabled={l.id === job.template}
                      onClick={() => relayout.mutate(l.id)}
                    >
                      <span className="flex flex-col">
                        <span>{l.name}</span>
                        <span className="text-xs text-text-faint">
                          {l.sections.length
                            ? l.sections.map((s) => s.heading).join(", ")
                            : "Topics chosen from the discussion"}
                        </span>
                      </span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
            )}
            {job.summary && (
              <DropdownMenuItem
                disabled={addToPage.isPending}
                onClick={() => addToPage.mutate()}
              >
                Add notes to page
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => revealTranscript()}>
              Go to the transcript
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setPane({ kind: "ask" })}>
              Open the meeting library
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {(turns.length > 0 || (audioAvailable && length > 0)) && (
        <div className="mt-5">
          <MeetingMap
            pageId={pageId}
            turns={turns}
            duration={length}
            colours={colours}
            marks={job.summary ? mapMarks(job.summary) : []}
            audio={audioAvailable}
          />
        </div>
      )}
      {talkers.length > 0 && (
        <div className="mt-4 flex flex-wrap items-center gap-2">
          {talkers.map((t) => (
            <button
              key={t.speaker}
              onClick={() => setNaming(true)}
              title="Name speakers"
              className="inline-flex items-center gap-1.5 rounded-full border border-border bg-bg-subtle px-2.5 py-1 text-xs text-text hover:border-border-strong"
            >
              <span
                className="size-2 rounded-full"
                style={{ background: colours.get(t.speaker) }}
                aria-hidden
              />
              {t.speaker}
              <span className="tabular-nums text-text-faint">{t.share}%</span>
            </button>
          ))}
        </div>
      )}

      {(busy || job.state === "queued" || job.error || regenerate.error) && (
        <div className="mt-5 space-y-2">
          {busy && (
            <p role="status" className="flex items-center gap-2 text-sm text-text-muted">
              <Loader2 className="size-3.5 animate-spin" />
              {job.state === "summarising"
                ? "Writing the notes on this Mac…"
                : "Preparing meeting search…"}
            </p>
          )}
          {job.state === "queued" && (
            <p role="status" className="flex items-center gap-2 text-sm text-text-muted">
              <Loader2 className="size-3.5 animate-spin" /> Transcript saved.
              Waiting for local AI to write the notes.
            </p>
          )}
          {job.error && (
            <p role="alert" className="text-sm text-danger-c">
              {job.error} Open the meeting library to retry.
            </p>
          )}
          {regenerate.error && (
            <p role="alert" className="text-sm text-danger-c">
              {errorMessage(regenerate.error)}
            </p>
          )}
        </div>
      )}
      {suggested.length > 0 && (
        <div className="mt-5 flex flex-wrap items-center gap-3 rounded-xl bg-brand-soft px-4 py-3 text-sm text-text">
          <span className="flex-1">
            From the conversation,{" "}
            {suggested
              .map((s) => `${s.label} sounds like ${s.suggestion?.name}`)
              .join(", ")}
            .
          </span>
          <Button size="sm" variant="outline" onClick={() => setNaming(true)}>
            Review names
          </Button>
        </div>
      )}

      {job.summary && (
        <div className="mt-7">
          <MeetingNotes
            summary={job.summary}
            onTime={jump}
            onDone={(action, done) => tick.mutate({ action, done })}
            colours={colours}
          />
          <div className="mt-7 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-border pt-4 text-xs text-text-faint">
            <span className="flex-1">
              Written on this Mac. Check names, dates and commitments against the
              transcript.
            </span>
            {!!job.summary.action_items.length && (
              <DropdownMenu>
                <DropdownMenuTrigger
                  disabled={toTasks.isPending}
                  render={<Button variant="ghost" size="sm" />}
                >
                  {toTasks.isPending && <Loader2 className="size-3 animate-spin" />}
                  Send next steps to planner
                  <ChevronDown className="size-3" />
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-56">
                  <p className="px-2 py-1 text-xs font-medium text-text-faint">
                    Add as tasks in
                  </p>
                  {databases.data?.map((db) => (
                    <DropdownMenuItem
                      key={db.database_id}
                      onClick={() =>
                        toTasks.mutate({ id: db.database_id, title: db.title })
                      }
                    >
                      {db.icon && <span>{db.icon}</span>}
                      {db.title || "Untitled"}
                    </DropdownMenuItem>
                  ))}
                  {databases.data?.length === 0 && (
                    <p className="px-2 py-1.5 text-xs text-text-muted">
                      Create a table for tasks first.
                    </p>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            )}
            <Button variant="ghost" size="sm" onClick={() => revealTranscript()}>
              Transcript
              <ArrowDown className="size-3" />
            </Button>
          </div>
        </div>
      )}
      <SpeakerNamesDialog
        pageId={pageId}
        open={naming}
        onOpenChange={setNaming}
        audioAvailable={audioAvailable}
      />
    </section>
  );
}
