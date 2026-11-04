import { UpdateSettings } from "@/features/updates/UpdateProvider";
import { RecordingSettings } from "./RecordingSettings";
import { CalendarSettings } from "./CalendarSettings";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Download, Circle, RefreshCw, Lock } from "lucide-react";
import { useUi, type Theme } from "@/store/ui";
import { diarizeApi, vaultApi, mcpApi } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { TranscriptionModels } from "./TranscriptionModels";

import { LocalAiSettings } from "./LocalAiSettings";

function Section({
  title,
  desc,
  children,
}: {
  title: string;
  desc?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-border py-8">
      <h2 className="text-base font-semibold">{title}</h2>
      {desc && <p className="mt-0.5 text-note text-text-muted">{desc}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

export function SettingsView() {
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-10 sm:px-10">
        <p className="page-eyebrow">Tidy</p>
        <h1 className="page-title">Settings</h1>
        <p className="page-description">
          Appearance, recording, local AI and updates.
        </p>

        <Section title="Appearance">
          <div className="inline-flex rounded-lg border border-border bg-surface p-0.5">
            {(["light", "dark", "system"] as Theme[]).map((t) => (
              <button
                key={t}
                onClick={() => setTheme(t)}
                className={`rounded-md px-3 py-1 text-sm capitalize transition-colors ${
                  theme === t
                    ? "bg-brand text-brand-fg"
                    : "text-text-muted hover:text-text"
                }`}
              >
                {t}
              </button>
            ))}
          </div>
        </Section>

        <Section title="Recording"><RecordingSettings /></Section>
        <Section title="Calendar"><CalendarSettings /></Section>

        <Section
          title="Transcription models"
          desc="Speech-to-text runs entirely on your Mac. Download a model to enable transcription."
        >
          <TranscriptionModels />
        </Section>

        <Section
          title="Local AI"
          desc="Set up private summaries and meeting search, and manage the models stored on this Mac."
        >
          <LocalAiSettings />
        </Section>

        <Section
          title="Speaker labels"
          desc="Add speaker labels to transcripts. The models run on this Mac and use about 35 MB."
        >
          <DiarizationSettings />
        </Section>

        <Section
          title="Markdown vault"
          desc="Mirror your pages to portable Markdown files you can open and edit in Obsidian, Finder, or git. Edits flow back in."
        >
          <VaultSettings />
        </Section>

        <Section
          title="Agent connection (MCP)"
          desc="Let Claude Code or Codex read and file notes into this workspace via a local MCP server. Read access is always on; you can enable write access below."
        >
          <McpSettings />
        </Section>

        <Section title="Updates"><UpdateSettings /></Section>

        <Section title="Privacy & data">
          <div className="flex items-start gap-2.5 rounded-lg bg-brand-soft px-4 py-3 text-sm text-text-muted">
            <Lock className="mt-0.5 size-4 shrink-0" />
            <span>
              Your notes and transcripts stay on this Mac. Audio is only kept if you enable it. Model downloads use Hugging Face; update checks and downloads use GitHub. Meeting content is never sent with these requests. AI processing runs locally.
            </span>
          </div>
        </Section>
      </div>
    </div>
  );
}

function DiarizationSettings() {
  const enabled = useUi((s) => s.diarizeEnabled);
  const setEnabled = useUi((s) => s.setDiarizeEnabled);
  const { data: available, error, refetch } = useQuery({
    queryKey: ["diarization-available"],
    queryFn: diarizeApi.available,
  });
  const download = useMutation({
    mutationFn: () => diarizeApi.download(),
    onSuccess: () => refetch(),
  });
  const downloading = download.isPending;

  return (
    <div className="space-y-3">
      <label className="flex items-center justify-between">
        <span className="text-sm">Label speakers in meeting transcripts</span>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </label>
      <div className="flex items-center gap-3">
        <span
          className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-note font-medium ${
            available
              ? "bg-[color-mix(in_srgb,var(--success)_14%,transparent)] text-success"
              : "bg-surface-hover text-text-muted"
          }`}
        >
          <Circle
            className={`size-2.5 ${available ? "fill-success" : "fill-text-faint"}`}
          />
          {available ? "Models installed" : "Models not installed"}
        </span>
        {!available && (
          <Button
            size="sm"
            variant="secondary"
            onClick={() => download.mutate()}
            disabled={downloading}
          >
            <Download className="size-3.5" />{" "}
            {downloading ? "Downloading…" : "Download models"}
          </Button>
        )}
      </div>
      {(download.error || error) && (
        <p role="alert" className="text-sm text-danger-c">
          {errorMessage(download.error || error)}
        </p>
      )}
    </div>
  );
}

function VaultSettings() {
  const qc = useQueryClient();
  const { data: dir } = useQuery({
    queryKey: ["vault-dir"],
    queryFn: () => vaultApi.getDir(),
  });
  const [path, setPath] = useState("");
  const [msg, setMsg] = useState<string | null>(null);

  const setDir = useMutation({
    mutationFn: (p: string) => vaultApi.setDir(p),
    onSuccess: (count) => {
      setMsg(`Vault linked · exported ${count} page${count === 1 ? "" : "s"}`);
      qc.invalidateQueries({ queryKey: ["vault-dir"] });
    },
  });
  const exportAll = useMutation({
    mutationFn: () => vaultApi.exportAll(),
    onSuccess: (count) =>
      setMsg(`Re-exported ${count} page${count === 1 ? "" : "s"}`),
  });
  const vaultError = setDir.error || exportAll.error;

  return (
    <div className="space-y-3">
      {dir ? (
        <div className="flex items-center gap-3">
          <code className="min-w-0 flex-1 truncate rounded bg-surface px-2.5 py-1.5 text-xs text-text-muted">
            {dir}
          </code>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => exportAll.mutate()}
            disabled={exportAll.isPending}
          >
            <RefreshCw
              className={`size-3.5 ${exportAll.isPending ? "animate-spin" : ""}`}
            />{" "}
            Re-export
          </Button>
        </div>
      ) : (
        <p className="text-note text-text-muted">No vault linked yet.</p>
      )}
      <div className="flex items-center gap-2">
        <input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="/absolute/path/to/vault-folder"
          className="min-w-0 flex-1 rounded-md border border-border bg-surface px-2.5 py-1.5 text-sm outline-none focus:ring-2 focus:ring-ring"
        />
        <Button
          size="sm"
          onClick={() => path.trim() && setDir.mutate(path.trim())}
          disabled={setDir.isPending || !path.trim()}
        >
          {dir ? "Change" : "Link vault"}
        </Button>
      </div>
      {vaultError && (
        <p role="alert" className="text-note text-danger-c">
          {errorMessage(vaultError)}
        </p>
      )}
      {msg && !vaultError && (
        <p role="status" className="text-note text-text-muted">
          {msg}
        </p>
      )}
    </div>
  );
}

function McpSettings() {
  const qc = useQueryClient();
  const { data: token } = useQuery({
    queryKey: ["mcp-token"],
    queryFn: () => mcpApi.getToken(),
  });
  const [copied, setCopied] = useState(false);

  // While write access is on, ask the core for the connector details so the
  // registration command is always on screen, not only in the session that
  // switched it on. `mcp_enable` is idempotent and reuses the saved token.
  const info = useQuery({
    queryKey: ["mcp-info"],
    queryFn: () => mcpApi.enable(),
    enabled: !!token,
    staleTime: Infinity,
  });
  const enable = useMutation({
    mutationFn: () => mcpApi.enable(),
    onSuccess: (i) => {
      disable.reset();
      qc.setQueryData(["mcp-info"], i);
      qc.invalidateQueries({ queryKey: ["mcp-token"] });
    },
  });
  const disable = useMutation({
    mutationFn: () => mcpApi.disable(),
    onSuccess: () => {
      // Forget the earlier "enabled" result too, or the screen keeps saying
      // writes are on and keeps showing the revoked command.
      enable.reset();
      qc.setQueryData(["mcp-token"], null);
      qc.removeQueries({ queryKey: ["mcp-info"] });
      qc.invalidateQueries({ queryKey: ["mcp-token"] });
    },
  });

  // The saved token is the truth; a just-finished enable covers the moment
  // before it is fetched again.
  const enabled = !!token || enable.isSuccess;
  const cmd = enabled
    ? (info.data?.claude_command ?? enable.data?.claude_command)
    : undefined;
  const error = enable.error || disable.error || info.error;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <span
          className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-note font-medium ${
            enabled
              ? "bg-[color-mix(in_srgb,var(--success)_14%,transparent)] text-success"
              : "bg-surface-hover text-text-muted"
          }`}
        >
          <Circle
            className={`size-2.5 ${enabled ? "fill-success" : "fill-text-faint"}`}
          />
          {enabled ? "Write access enabled" : "Read-only"}
        </span>
        {enabled ? (
          <Button
            size="sm"
            variant="secondary"
            onClick={() => disable.mutate()}
            disabled={disable.isPending}
          >
            Revoke writes
          </Button>
        ) : (
          <Button
            size="sm"
            onClick={() => enable.mutate()}
            disabled={enable.isPending}
          >
            Enable writes
          </Button>
        )}
      </div>
      {error && (
        <p role="alert" className="text-note text-danger-c">
          {errorMessage(error)}
        </p>
      )}
      {enabled && info.isPending && !cmd && (
        <p role="status" className="text-note text-text-muted">
          Loading the connector command…
        </p>
      )}
      {cmd && (
        <div className="space-y-1.5">
          <p className="text-note text-text-muted">
            Register the connector by running this once in your terminal:
          </p>
          <div className="flex items-start gap-2">
            <code className="min-w-0 flex-1 overflow-x-auto whitespace-pre rounded bg-surface px-2.5 py-2 text-xs text-text">
              {cmd}
            </code>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                navigator.clipboard?.writeText(cmd);
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              }}
            >
              {copied ? <Check className="size-3.5" /> : "Copy"}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
