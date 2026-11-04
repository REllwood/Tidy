import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { meetingBriefApi, meetingNotesApi } from "@/lib/api";
import { meetingJobsKey } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { flushPendingEdits } from "@/lib/pendingEdits";
import { announceDocumentChange } from "@/hooks/useDocument";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

/** Put names to "Speaker 1", "Speaker 2"… across the transcript and notes. */
export function SpeakerNamesDialog({
  pageId,
  open,
  onOpenChange,
}: {
  pageId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const qc = useQueryClient();
  const speakers = useQuery({
    queryKey: ["meeting-speakers", pageId],
    queryFn: () => meetingNotesApi.speakers(pageId),
    enabled: open,
  });
  // People invited to the meeting, offered as names to pick from.
  const event = useQuery({
    queryKey: ["meeting-event", pageId],
    queryFn: () => meetingBriefApi.event(pageId),
    enabled: open,
  });
  const invited = (event.data?.attendees ?? []).filter((a) => !a.includes("@"));
  const [names, setNames] = useState<Record<string, string>>({});
  useEffect(() => {
    if (open) setNames({});
  }, [open]);
  const changes = Object.entries(names)
    .map(([from, to]) => ({ from, to: to.trim() }))
    .filter(({ from, to }) => to && to !== from);
  const save = useMutation({
    mutationFn: async () => {
      // The open editor may hold an unsaved edit; save it before the page
      // is rewritten underneath it.
      await flushPendingEdits();
      return meetingNotesApi.renameSpeakers(pageId, changes);
    },
    onSuccess: (changed) => {
      void qc.invalidateQueries({ queryKey: meetingJobsKey });
      void qc.invalidateQueries({ queryKey: ["meeting-speakers", pageId] });
      announceDocumentChange(qc, pageId);
      if (changed) toast.success("Speakers named in the transcript and notes");
      onOpenChange(false);
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !save.isPending && onOpenChange(o)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Name the speakers</DialogTitle>
          <DialogDescription>
            Names replace the speaker labels throughout the transcript and in
            the notes, so answers can say who said what.
          </DialogDescription>
        </DialogHeader>
        {speakers.isPending && (
          <p role="status" className="flex items-center gap-2 text-sm text-text-muted">
            <Loader2 className="size-4 animate-spin" /> Reading the transcript…
          </p>
        )}
        {speakers.data?.length === 0 && (
          <p className="text-sm text-text-muted">
            This transcript doesn't mark who is speaking.
          </p>
        )}
        <div className="max-h-[50vh] space-y-4 overflow-y-auto">
          {speakers.data?.map((speaker) => (
            <label key={speaker.label} className="block space-y-1.5">
              <span className="flex items-baseline justify-between gap-3 text-sm font-medium">
                {speaker.label}
                <span className="text-xs font-normal text-text-faint">
                  {speaker.turns} {speaker.turns === 1 ? "turn" : "turns"}
                </span>
              </span>
              {speaker.sample && (
                <span className="block text-xs italic text-text-muted">
                  “{speaker.sample}”
                </span>
              )}
              <input
                list={invited.length ? "speaker-invitees" : undefined}
                value={names[speaker.label] ?? ""}
                onChange={(e) =>
                  setNames((n) => ({ ...n, [speaker.label]: e.target.value }))
                }
                maxLength={40}
                placeholder="Name"
                aria-label={`Name for ${speaker.label}`}
                className="w-full rounded-md border border-border bg-surface px-3 py-2 text-sm outline-none focus:border-brand"
              />
            </label>
          ))}
        </div>
        {invited.length > 0 && (
          <datalist id="speaker-invitees">
            {invited.map((name) => (
              <option key={name} value={name} />
            ))}
          </datalist>
        )}
        {(speakers.error || save.error) && (
          <p role="alert" className="text-sm text-danger-c">
            {errorMessage(speakers.error || save.error)}
          </p>
        )}
        <DialogFooter>
          <Button
            variant="secondary"
            disabled={save.isPending}
            onClick={() => onOpenChange(false)}
          >
            Cancel
          </Button>
          <Button
            disabled={!changes.length || save.isPending}
            onClick={() => save.mutate()}
          >
            {save.isPending && <Loader2 className="size-4 animate-spin" />}
            Save names
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
