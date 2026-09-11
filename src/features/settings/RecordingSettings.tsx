import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { recordingPreferencesApi } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { Switch } from "@/components/ui/switch";
export function RecordingSettings() {
  const qc = useQueryClient();
  const saved = useQuery({
    queryKey: ["recording-preferences"],
    queryFn: recordingPreferencesApi.get,
  });
  const change = useMutation({
    mutationFn: recordingPreferencesApi.set,
    onSuccess: () =>
      qc.invalidateQueries({ queryKey: ["recording-preferences"] }),
  });
  return (
    <div className="space-y-3">
      <label className="flex items-center justify-between gap-4 text-sm">
        <span>Keep meeting audio</span>
        <Switch
          checked={saved.data ?? false}
          disabled={saved.isPending || saved.isError || change.isPending}
          onCheckedChange={(v) => change.mutate(v)}
        />
      </label>
      <p className="text-sm text-text-muted">
        Off by default. Transcripts are saved separately. Turn this on to keep
        the audio so you can listen to it or transcribe it again.
      </p>
      <p className="text-xs text-text-muted">
        Applies to new recordings. When off, temporary audio is discarded after
        processing, including if transcription fails. Existing saved audio is
        kept. Audio uses about 115 MB per hour.
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
    </div>
  );
}
