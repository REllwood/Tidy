import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Loader2 } from "lucide-react";
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
  meetingBriefApi,
  meetingNotesApi,
  recordingHistoryApi,
  recordingHistoryKey,
  type SavedRecording,
} from "@/lib/api";
import { flushPendingEdits } from "@/lib/pendingEdits";
import { announceDocumentChange } from "@/hooks/useDocument";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SpeakerNamesDialog } from "./SpeakerNames";
import { useUi } from "@/store/ui";

import { MeetingNotes } from "./MeetingNotes";
import { Button } from "@/components/ui/button";
import { errorMessage } from "@/lib/errors";
import { isMeetingPage, summaryPanelInterval } from "./polling";

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
  const job = data?.find((j) => j.page_id === pageId);
  if (!job) return null;
  const layout = layouts.data?.find((l) => l.id === job.template);
  const busy = ["summarising", "indexing"].includes(job.state);
  return (
    <section
      className="my-6 rounded-xl border border-border bg-surface p-6 sm:p-8"
      aria-label="Generated meeting summary"
    >
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">
          Meeting notes
          {layout && layout.id !== "general" && (
            <span className="ml-2 font-normal text-text-faint">
              {layout.name} layout
            </span>
          )}
        </h2>
        <button
          className="text-xs text-brand hover:underline"
          onClick={() => setPane({ kind: "ask" })}
        >
          Meeting library
        </button>
      </div>
      {busy && (
        <p
          role="status"
          className="mt-2 flex items-center gap-2 text-xs text-text-muted"
        >
          <Loader2 className="size-3 animate-spin" />
          {job.state === "summarising"
            ? "Summarising locally…"
            : "Preparing meeting search…"}
        </p>
      )}
      {job.state === "queued" && (
        <p
          role="status"
          className="mt-2 flex items-center gap-2 text-sm text-text-muted"
        >
          <Loader2 className="size-3 animate-spin" /> Transcript saved. Waiting
          for local AI to process this meeting.
        </p>
      )}
      {job.error && (
        <p role="alert" className="mt-2 text-sm text-danger-c">
          {job.error} Open the meeting library to retry.
        </p>
      )}
      {regenerate.error && (
        <p role="alert" className="mt-3 text-sm text-danger-c">
          {errorMessage(regenerate.error)}
        </p>
      )}
      {job.summary && (
        <div className="mt-3 space-y-3 text-sm leading-relaxed">
          <MeetingNotes summary={job.summary} />
          <p className="text-xs text-text-faint">
            Generated locally. Check names, dates and commitments against the
            original transcript.
          </p>
        </div>
      )}
      <div className="mt-5 flex flex-wrap gap-2 border-t border-border pt-4">
        <Button
          variant="secondary"
          size="sm"
          disabled={
            regenerate.isPending ||
            busy ||
            job.state === "queued" ||
            !status.data?.ready ||
            // Background work can keep running; only a setup or download the
            // person started here blocks a new request.
            isUserProgress(status.data.progress) ||
            status.data.recording_paused
          }
          onClick={() => regenerate.mutate()}
        >
          {regenerate.isPending && <Loader2 className="size-3 animate-spin" />}
          Regenerate notes
        </Button>
        {job.summary && (
          <Button
            variant="secondary"
            size="sm"
            disabled={addToPage.isPending}
            onClick={() => addToPage.mutate()}
          >
            {addToPage.isPending && <Loader2 className="size-3 animate-spin" />}
            Add notes to page
          </Button>
        )}
        {!!job.summary?.action_items.length && (
          <DropdownMenu>
            <DropdownMenuTrigger
              disabled={toTasks.isPending}
              render={<Button variant="secondary" size="sm" />}
            >
              {toTasks.isPending && <Loader2 className="size-3 animate-spin" />}
              Send next steps to planner
              <ChevronDown className="size-3" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-56">
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
        {!!layouts.data?.length && (
          <DropdownMenu>
            <DropdownMenuTrigger
              disabled={
                relayout.isPending ||
                busy ||
                job.state === "queued" ||
                !status.data?.ready
              }
              render={<Button variant="ghost" size="sm" />}
            >
              {relayout.isPending && <Loader2 className="size-3 animate-spin" />}
              Rewrite notes as
              <ChevronDown className="size-3" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-64">
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
            </DropdownMenuContent>
          </DropdownMenu>
        )}
        <Button variant="ghost" size="sm" onClick={() => setNaming(true)}>
          Name speakers
        </Button>
      </div>
      <SpeakerNamesDialog
        pageId={pageId}
        open={naming}
        onOpenChange={setNaming}
      />
    </section>
  );
}
