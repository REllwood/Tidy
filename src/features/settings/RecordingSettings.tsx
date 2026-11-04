import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { meetingAutomationApi, recordingPreferencesApi } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";

const KEY = ["recording-preferences"] as const;
const VOCABULARY_KEY = ["transcription-vocabulary"] as const;
const AUTOMATION_KEY = ["meeting-automation"] as const;

/** Offering to record calls, and stopping recordings once a meeting ends. */
function Automation() {
  const qc = useQueryClient();
  const saved = useQuery({
    queryKey: AUTOMATION_KEY,
    queryFn: meetingAutomationApi.get,
  });
  const change = useMutation({
    mutationFn: ([offers, autoStop]: [boolean, boolean]) =>
      meetingAutomationApi.set(offers, autoStop),
    onMutate: async (next) => {
      await qc.cancelQueries({ queryKey: AUTOMATION_KEY });
      const previous = qc.getQueryData<[boolean, boolean]>(AUTOMATION_KEY);
      qc.setQueryData(AUTOMATION_KEY, next);
      return { previous };
    },
    onError: (_error, _next, context) => {
      if (context?.previous) qc.setQueryData(AUTOMATION_KEY, context.previous);
    },
    onSettled: () => qc.invalidateQueries({ queryKey: AUTOMATION_KEY }),
  });
  const [offers, autoStop] = saved.data ?? [true, true];
  const disabled = saved.isPending || saved.isError;
  return (
    <div className="space-y-3 border-t border-border pt-3">
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>Offer to record calls</span>
        <Switch
          checked={offers}
          disabled={disabled}
          onCheckedChange={(v) => change.mutate([v, autoStop])}
        />
      </label>
      <p className="text-xs text-text-muted">
        When Zoom, Teams, FaceTime, Slack or a browser starts using the
        microphone, a small window asks whether to record. Nothing records
        until you choose Record.
      </p>
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>Stop when the meeting ends</span>
        <Switch
          checked={autoStop}
          disabled={disabled}
          onCheckedChange={(v) => change.mutate([offers, v])}
        />
      </label>
      <p className="text-xs text-text-muted">
        Stops when the call ends, after 3 minutes of silence, or 5 minutes
        after the calendar event ends. A 15-second countdown lets you keep
        recording.
      </p>
      {(saved.error || change.error) && (
        <p role="alert" className="text-sm text-danger-c">
          {errorMessage(saved.error || change.error)}
        </p>
      )}
    </div>
  );
}

/** Names and terms Whisper should expect, so they are spelt the right way. */
function Vocabulary() {
  const qc = useQueryClient();
  const saved = useQuery({
    queryKey: VOCABULARY_KEY,
    queryFn: recordingPreferencesApi.vocabulary,
  });
  const [draft, setDraft] = useState("");
  useEffect(() => {
    if (saved.data !== undefined) setDraft(saved.data);
  }, [saved.data]);
  const save = useMutation({
    mutationFn: recordingPreferencesApi.setVocabulary,
    onSuccess: () => toast.success("Vocabulary saved"),
    onSettled: () => qc.invalidateQueries({ queryKey: VOCABULARY_KEY }),
  });
  const changed = saved.data !== undefined && draft.trim() !== saved.data.trim();
  return (
    <div className="space-y-2 border-t border-border pt-3">
      <label htmlFor="transcription-vocabulary" className="text-sm">
        Names and terms
      </label>
      <Textarea
        id="transcription-vocabulary"
        value={draft}
        disabled={saved.isPending || saved.isError}
        placeholder="Harbourside Dental, Cliniko, Dr Chen"
        rows={3}
        onChange={(e) => setDraft(e.target.value)}
      />
      <p className="text-xs text-text-muted">
        Separate them with commas or new lines. Transcription uses these, and
        the client a meeting is filed under, to spell names the way you do.
      </p>
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={!changed || save.isPending}
          onClick={() => save.mutate(draft)}
        >
          {save.isPending ? "Saving…" : "Save"}
        </Button>
        {(saved.error || save.error) && (
          <p role="alert" className="text-sm text-danger-c">
            {errorMessage(saved.error || save.error)}
          </p>
        )}
      </div>
    </div>
  );
}

export function RecordingSettings() {
  const qc = useQueryClient();
  const saved = useQuery({
    queryKey: KEY,
    queryFn: recordingPreferencesApi.get,
  });
  const change = useMutation({
    mutationFn: recordingPreferencesApi.set,
    // Flip the switch straight away, then put it back if the core refuses.
    onMutate: async (keepAudio: boolean) => {
      await qc.cancelQueries({ queryKey: KEY });
      const previous = qc.getQueryData<boolean>(KEY);
      qc.setQueryData<boolean>(KEY, keepAudio);
      return { previous };
    },
    onError: (_error, _keepAudio, context) => {
      if (context && context.previous !== undefined)
        qc.setQueryData<boolean>(KEY, context.previous);
    },
    onSettled: () => qc.invalidateQueries({ queryKey: KEY }),
  });
  return (
    <div className="space-y-3">
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>Keep meeting audio</span>
        <Switch
          checked={saved.data ?? false}
          disabled={saved.isPending || saved.isError}
          onCheckedChange={(v) => change.mutate(v)}
        />
      </label>
      <p className="text-sm text-text-muted">
        Off by default. Transcripts are saved separately. Turn this on to keep
        the audio so you can listen to it or transcribe it again.
      </p>
      <p className="text-xs text-text-muted">
        Applies to new recordings. When off, audio is discarded once a
        transcript has been saved. A meeting that has not been transcribed keeps
        its audio so you can try again, and you can discard it from Recording
        history. Existing saved audio is kept. Audio uses about 230 MB per hour,
        because your microphone and the other side of a call are kept apart.
      </p>
      {(saved.isPending || change.isPending) && (
        <p role="status" className="flex items-center gap-2 text-sm">
          <Loader2 className="size-4 animate-spin" />
          {change.isPending ? "Saving…" : "Loading…"}
        </p>
      )}
      {(saved.error || change.error) && (
        <p role="alert" className="text-sm text-danger-c">
          {errorMessage(saved.error || change.error)}
        </p>
      )}
      <Automation />
      <Vocabulary />
    </div>
  );
}
