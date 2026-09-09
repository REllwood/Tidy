import { errorMessage } from "@/lib/errors";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  localAi,
  aiStatusKey,
  type AiConfig,
  type AiProgress,
} from "@/lib/localAi";
import { ollamaApi } from "@/lib/api";
import { isTauri } from "@/lib/tauri";

const size = (bytes: number) =>
  bytes >= 1e9
    ? `${(bytes / 1e9).toFixed(1)} GB`
    : `${Math.round(bytes / 1e6)} MB`;
export function AiProgressView({ progress }: { progress: AiProgress }) {
  const percent = progress.total
    ? Math.min(100, Math.round((progress.completed / progress.total) * 100))
    : null;
  return (
    <div
      role="status"
      aria-live="polite"
      className="space-y-2 rounded-lg bg-bg-subtle p-3"
    >
      <p className="flex items-center gap-2 text-sm">
        <Loader2 className="size-4 animate-spin" aria-hidden="true" />
        {progress.label}
        {percent !== null && ` · ${percent}%`}
      </p>
      {percent !== null && (
        <progress
          className="h-1.5 w-full accent-[var(--brand)]"
          value={percent}
          max={100}
          aria-label={progress.label}
        />
      )}
    </div>
  );
}
export function LocalAiSettings() {
  const qc = useQueryClient();
  const status = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
    refetchInterval: 1500,
  });
  const [removeId, setRemoveId] = useState<string | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [ollamaModel, setOllamaModel] = useState("");
  const ollama = useQuery({
    queryKey: ["ollama-status"],
    queryFn: ollamaApi.status,
    enabled: showAdvanced,
  });
  const action = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => {
      setRemoveId(null);
      qc.invalidateQueries({ queryKey: aiStatusKey });
    },
  });
  const cancel = useMutation({ mutationFn: localAi.cancel });
  const data = status.data;
  if (status.isPending)
    return (
      <p role="status" className="flex items-center gap-2 text-sm">
        <Loader2 className="size-4 animate-spin" />
        Checking local AI…
      </p>
    );
  if (!data)
    return (
      <p role="alert" className="text-sm text-danger-c">
        Could not load AI settings: {errorMessage(status.error)}
        <Button
          className="ml-2"
          size="sm"
          disabled={status.isFetching}
          onClick={() => status.refetch()}
        >
          {status.isFetching && <Loader2 className="size-3 animate-spin" />}
          Retry
        </Button>
      </p>
    );
  const setupBytes = data.models
    .filter(
      (m) =>
        !m.downloaded &&
        [data.recommended_model_id, "nomic-embed"].includes(m.id),
    )
    .reduce((sum, m) => sum + m.size, 0);
  const busy =
    action.isPending ||
    !!data.progress ||
    data.recording_paused ||
    status.isError;
  const configure = (config: AiConfig) =>
    action.mutate(() => localAi.configure(config));
  return (
    <div className="space-y-5">
      {status.error && (
        <p role="alert" className="text-sm text-danger-c">
          Could not refresh local AI: {errorMessage(status.error)}
        </p>
      )}
      {data.recording_paused && (
        <p
          role="status"
          className="rounded-lg border border-border bg-brand-soft p-3 text-sm text-text-muted"
        >
          Local AI is paused while recording or transcribing. Your installed
          models are still available below. Finish the meeting, then retry any
          interrupted download.
        </p>
      )}
      {!isTauri() && (
        <p className="text-xs text-text-faint">
          Browser preview · setup and answers are demonstrations.
        </p>
      )}
      <div className="rounded-xl border border-border bg-surface p-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="text-sm font-semibold">
              {data.ready
                ? "Local AI is enabled"
                : "Your meetings, understood on your Mac"}
            </h3>
            <p className="mt-1 text-sm leading-relaxed text-text-muted">
              Automatic summaries and answers grounded in your transcripts. No
              account or separate app required.
            </p>
          </div>
          <span className="shrink-0 rounded-full bg-bg-subtle px-2 py-1 text-xs text-text-muted">
            {data.ready ? "Enabled" : "Optional"}
          </span>
        </div>
        {!data.runtime_available && (
          <p role="alert" className="mt-3 text-sm text-danger-c">
            This build is missing the bundled AI runtime. Install a complete
            Tidy release.
          </p>
        )}
        {!data.ready && (
          <>
            <p className="mt-3 text-xs leading-relaxed text-text-faint">
              Downloads {size(setupBytes)} from Hugging Face.{" "}
              {data.memory_bytes
                ? `This Mac has ${Math.round(data.memory_bytes / 1024 ** 3)} GB RAM; ${data.recommended_model_id === "qwen3-small" ? "the lightweight model is recommended" : "the balanced model is recommended"}.`
                : "The balanced model is recommended for Macs with 16 GB RAM."}{" "}
              Once downloaded, your meeting text stays on this Mac.
            </p>
            <Button
              className="mt-4"
              disabled={busy || !data.runtime_available}
              onClick={() => action.mutate(localAi.setup)}
            >
              {action.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : null}
              Enable local AI
            </Button>
          </>
        )}
        {data.ready && (
          <Button
            className="mt-4"
            variant="secondary"
            size="sm"
            disabled={busy}
            onClick={() => configure({ ...data.config, provider: "disabled" })}
          >
            Turn off automatic AI
          </Button>
        )}
      </div>
      {data.progress ? (
        <div className="space-y-2">
          <AiProgressView progress={data.progress} />
          <Button
            size="sm"
            variant="secondary"
            disabled={cancel.isPending}
            onClick={() => cancel.mutate()}
          >
            {cancel.isPending && <Loader2 className="size-3.5 animate-spin" />}
            Cancel current operation
          </Button>
        </div>
      ) : (
        action.isPending && (
          <p role="status" className="flex gap-2 text-sm">
            <Loader2 className="size-4 animate-spin" />
            Updating local AI…
          </p>
        )
      )}
      {(action.error || cancel.error) && (
        <p role="alert" className="text-sm text-danger-c">
          {errorMessage(action.error || cancel.error)}
        </p>
      )}
      <div>
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-medium">Models on this Mac</h3>
          <span className="text-xs text-text-faint">
            {size(data.disk_bytes)} used, including partial downloads
          </span>
        </div>
        <div className="divide-y divide-border rounded-lg border border-border">
          {data.models.map((model) => (
            <div key={model.id} className="p-4">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium">{model.name}</span>
                <span className="text-xs text-text-faint">
                  {size(model.size)}
                </span>
                {data.config.provider === "builtin" &&
                  data.config.model_id === model.id && (
                    <span className="text-xs text-brand">Selected</span>
                  )}
              </div>
              <p className="mt-1 text-xs leading-relaxed text-text-muted">
                {model.description}
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                {!model.downloaded ? (
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy}
                    onClick={() =>
                      action.mutate(() => localAi.download(model.id))
                    }
                  >
                    Download
                  </Button>
                ) : (
                  <>
                    {model.purpose === "chat" &&
                      (data.config.provider !== "builtin" ||
                        data.config.model_id !== model.id) && (
                        <Button
                          size="sm"
                          variant="secondary"
                          disabled={busy}
                          onClick={() =>
                            configure({
                              ...data.config,
                              provider: "builtin",
                              model_id: model.id,
                            })
                          }
                        >
                          Use this model
                        </Button>
                      )}
                    <span className="self-center text-xs text-text-faint">
                      Installed
                    </span>
                  </>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => setRemoveId(model.id)}
                >
                  Remove files…
                </Button>
              </div>
              {removeId === model.id && (
                <div
                  role="group"
                  aria-label={`Confirm removal of ${model.name}`}
                  className="mt-3 rounded-md bg-bg-subtle p-3 text-sm"
                >
                  <p>
                    Remove {model.name} and its partial downloads from this Mac?
                    Your meetings will remain. A selected answer model will be
                    disabled.
                  </p>
                  <div className="mt-2 flex gap-2">
                    <Button
                      size="sm"
                      disabled={busy}
                      onClick={() =>
                        action.mutate(() => localAi.remove(model.id))
                      }
                    >
                      Confirm removal
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={busy}
                      onClick={() => setRemoveId(null)}
                    >
                      Keep files
                    </Button>
                  </div>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
      <div className="border-t border-border pt-4">
        <button
          className="text-sm text-text-muted underline underline-offset-4"
          aria-expanded={showAdvanced}
          onClick={() => setShowAdvanced(!showAdvanced)}
        >
          Advanced: use an existing Ollama installation
        </button>
        {showAdvanced && (
          <div className="mt-3 space-y-2">
            <p className="text-xs text-text-faint">
              Choose an installed local chat model. The Meeting search download
              is still required for questions. Cloud models are excluded.
            </p>
            {ollama.isFetching && (
              <p role="status" className="flex items-center gap-2 text-xs">
                <Loader2 className="size-3 animate-spin" />
                Checking Ollama…
              </p>
            )}
            <label className="block text-xs">
              Ollama model
              <select
                className="mt-1 block w-full rounded-md border border-border bg-surface p-2 text-sm"
                value={ollamaModel}
                onChange={(e) => setOllamaModel(e.target.value)}
              >
                <option value="">Select an installed model</option>
                {ollama.data?.models
                  .filter((m) => !m.includes("cloud"))
                  .map((m) => (
                    <option key={m}>{m}</option>
                  ))}
              </select>
            </label>
            {ollama.error && (
              <p role="alert" className="text-sm text-danger-c">
                {errorMessage(ollama.error)}
              </p>
            )}
            {ollama.data && !ollama.data.available && (
              <p className="text-xs text-text-muted">
                Ollama is not running on this Mac.
              </p>
            )}
            <Button
              variant="secondary"
              size="sm"
              disabled={busy || !ollamaModel}
              onClick={() =>
                configure({
                  ...data.config,
                  provider: "ollama",
                  ollama_model: ollamaModel,
                })
              }
            >
              Use Ollama model
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
