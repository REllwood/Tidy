import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Play, Sparkles } from "lucide-react";
import { toast } from "sonner";
import { meetingNotesApi } from "@/lib/api";
import { meetingJobsKey } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { flushPendingEdits } from "@/lib/pendingEdits";
import { announceDocumentChange } from "@/hooks/useDocument";
import { Button } from "@/components/ui/button";
import { shortStamp, usePlayback } from "./playback";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

function stamp(ms: number) {
  const s = Math.floor(ms / 1000);
  return shortStamp(`${Math.floor(s / 3600)}:${Math.floor(s / 60) % 60}:${s % 60}`);
}

/**
 * Put names to "Speaker 1", "Speaker 2"… across the transcript and notes.
 * Names the conversation points to are filled in, with the line that points
 * to them, and each speaker's lines can be played when the audio was kept.
 */
export function SpeakerNamesDialog({
  pageId,
  open,
  onOpenChange,
  audioAvailable = false,
}: {
  pageId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  audioAvailable?: boolean;
}) {
  const play = usePlayback((s) => s.play);
  const qc = useQueryClient();
  const speakers = useQuery({
    queryKey: ["meeting-speakers", pageId],
    queryFn: () => meetingNotesApi.speakers(pageId),
    enabled: open,
  });
  // People invited to the meeting or seen in the call, offered as names.
  const people = useQuery({
    queryKey: ["meeting-people", pageId],
    queryFn: () => meetingNotesApi.people(pageId),
    enabled: open,
  });
  const invited = people.data ?? [];
  const [names, setNames] = useState<Record<string, string>>({});
  useEffect(() => {
    if (open) setNames({});
  }, [open]);
  // Fill in what the conversation suggests, once, without overwriting typing.
  const suggested = (speakers.data ?? []).filter((s) => s.suggestion);
  useEffect(() => {
    if (!open || !suggested.length) return;
    setNames((current) => {
      const next = { ...current };
      for (const s of suggested)
        if (next[s.label] === undefined && s.suggestion) next[s.label] = s.suggestion.name;
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, speakers.data]);
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
            the notes, so answers can say who said what. Names the conversation
            points to, such as someone asked a question by name who then
            answered, are filled in for you to check.
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
              <span className="block space-y-0.5">
                {(speaker.samples?.length
                  ? speaker.samples
                  : speaker.sample
                    ? [{ at_ms: -1, text: speaker.sample }]
                    : []
                ).map((line, i) => (
                  <span key={i} className="flex items-start gap-1.5 text-xs text-text-muted">
                    {audioAvailable && line.at_ms >= 0 ? (
                      <button
                        type="button"
                        className="mt-0.5 shrink-0 rounded text-brand hover:bg-brand-soft"
                        onClick={() => play(pageId, line.at_ms / 1000)}
                        aria-label={`Play what ${speaker.label} said at ${stamp(line.at_ms)}`}
                        title="Listen"
                      >
                        <Play className="size-3" />
                      </button>
                    ) : null}
                    {line.at_ms >= 0 && (
                      <span className="shrink-0 font-mono text-text-faint">{stamp(line.at_ms)}</span>
                    )}
                    <span className="italic">“{line.text}”</span>
                  </span>
                ))}
              </span>
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
              {speaker.suggestion && (
                <span className="flex items-start gap-1.5 text-xs text-text-faint">
                  <Sparkles className="mt-0.5 size-3 shrink-0 text-brand" />
                  {speaker.suggestion.from_app ? (
                    <span>
                      Suggested {speaker.suggestion.name}: {speaker.suggestion.from_app} showed
                      them speaking while this voice talked, as in “{speaker.suggestion.because}” at{" "}
                      {stamp(speaker.suggestion.at_ms)}.
                    </span>
                  ) : (
                    <span>
                      Suggested {speaker.suggestion.name} from “{speaker.suggestion.because}” at{" "}
                      {stamp(speaker.suggestion.at_ms)}.
                    </span>
                  )}
                </span>
              )}
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
