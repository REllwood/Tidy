import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, Download, Loader2, Trash2 } from "lucide-react";
import { useModels, useModelMutations, useDownloadProgress, useModelDownloads } from "@/hooks/useModels";
import { aiStatusKey, localAi } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { MemoryGuidance, ModelRangeFilter, type ModelRange } from "./ModelGuidance";

export function TranscriptionModels() {
  const models = useModels();
  const hardware = useQuery({ queryKey: aiStatusKey, queryFn: localAi.status });
  const progress = useDownloadProgress();
  const downloads = useModelDownloads();
  const m = useModelMutations();
  const [range, setRange] = useState<ModelRange>("all");
  const [removeId, setRemoveId] = useState<string | null>(null);
  const busy = downloads.length > 0 || m.select.isPending || m.remove.isPending;
  const error = models.error || m.download.error || m.select.error || m.remove.error;
  if (models.isPending) return <p role="status" className="flex items-center gap-2 text-sm"><Loader2 className="size-4 animate-spin" />Loading transcription models…</p>;
  const visible = models.data?.filter((model) => range === "all" || model.tier === range) ?? [];
  return <div className="space-y-4">
    <p className="text-xs leading-relaxed text-text-muted">
      Choose one model for transcription. Smaller models generally finish sooner; larger models offer another option for difficult audio.
      RAM figures are estimates for the whole Mac, including room for other apps. Download size is storage space, not RAM use.
    </p>
    <ModelRangeFilter value={range} onChange={setRange} label="Transcription options" />
    {error && <p role="alert" className="text-sm text-danger-c">{errorMessage(error)} <Button size="sm" variant="secondary" disabled={models.isFetching} onClick={() => models.refetch()}>{models.isFetching && <Loader2 className="size-3 animate-spin" />}Refresh list</Button></p>}
    {downloads.length > 0 && <p role="status" className="flex items-center gap-2 text-sm"><Loader2 className="size-4 animate-spin" />Downloading transcription model. You can leave this screen.</p>}
    <div className="divide-y divide-border rounded-lg border border-border">
      {visible.map((model) => {
        const downloading = downloads.includes(model.id);
        const selecting = m.select.isPending && m.select.variables === model.id;
        const removing = m.remove.isPending && m.remove.variables === model.id;
        const pct = downloading && progress[model.id] != null ? Math.min(100, Math.round(progress[model.id] * 100)) : null;
        return <div key={model.id} className="p-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium">{model.name}</span>
            <span className="text-xs text-text-faint">{model.size >= 1e9 ? `${(model.size / 1e9).toFixed(1)} GB` : `${Math.round(model.size / 1e6)} MB`} download</span>
            {model.selected && <span className="inline-flex items-center gap-1 text-xs text-brand"><Check className="size-3" />Selected</span>}
          </div>
          <p className="mt-1 text-xs leading-relaxed text-text-muted">{model.description}</p>
          <MemoryGuidance suggested={model.recommended_ram_gb} memoryBytes={hardware.data?.memory_bytes} />
          <div className="mt-3 flex flex-wrap gap-2">
            {!model.downloaded ? <Button size="sm" variant="secondary" disabled={busy} aria-label={`${downloading ? "Downloading" : "Download"} ${model.name}`} onClick={() => m.download.mutate(model.id)}>
              {downloading ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
              {downloading ? `Downloading…${pct === null ? "" : ` ${pct}%`}` : "Download"}
            </Button> : <>
              {!model.selected && <Button size="sm" variant="secondary" disabled={busy} aria-label={`Select ${model.name}`} onClick={() => m.select.mutate(model.id)}>{selecting && <Loader2 className="size-3.5 animate-spin" />}{selecting ? "Selecting…" : "Select"}</Button>}
              <span className="self-center text-xs text-text-faint">Installed</span>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRemoveId(model.id)}><Trash2 className="size-3.5" />Remove…</Button>
            </>}
          </div>
          {downloading && pct !== null && <progress className="mt-3 h-1.5 w-full accent-[var(--brand)]" value={pct} max={100} aria-label={`Downloading ${model.name}`} />}
          {removeId === model.id && <div role="group" aria-label={`Remove ${model.name}`} className="mt-3 space-y-2 rounded-md bg-bg-subtle p-3 text-sm">
            <p>Remove {model.name} from this Mac? Saved transcripts will remain.</p>
            <div className="flex gap-2">
              <Button size="sm" disabled={busy} onClick={() => m.remove.mutate(model.id, {onSuccess: () => setRemoveId(null)})}>{removing && <Loader2 className="size-3.5 animate-spin" />}{removing ? "Removing…" : "Remove model"}</Button>
              <Button size="sm" variant="secondary" disabled={busy} onClick={() => setRemoveId(null)}>Keep model</Button>
            </div>
          </div>}
        </div>;
      })}
    </div>
    {visible.length === 0 && !models.error && <p className="text-sm text-text-muted">No models in this group.</p>}
  </div>;
}
