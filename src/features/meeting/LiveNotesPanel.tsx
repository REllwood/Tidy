import { Loader2, RefreshCw, Sparkles } from "lucide-react";
import type { AgentNote, LiveLine, LiveNotesBy, LiveView } from "@/lib/api";
import { useUi } from "@/store/ui";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useSharedLiveNotes } from "./useLiveNotes";

export const LIVE_VIEWS: { id: LiveView; label: string }[] = [
  { id: "summary", label: "Summary so far" },
  { id: "questions", label: "Questions to ask" },
  { id: "decisions", label: "Decisions and next steps" },
  { id: "open", label: "Still open" },
  { id: "custom", label: "Your own" },
];
const AGENT_VIEWS: Record<AgentNote["view"], string> = {
  summary: "Summary",
  questions: "Question to ask",
  decisions: "Decision or next step",
  open: "Still open",
  note: "Note",
};

function clock(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

function ago(ms: number) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  return m === 1 ? "1 minute ago" : `${m} minutes ago`;
}

/**
 * Notes that keep up with the meeting: a summary, questions to ask,
 * decisions and next steps, what is still open and the person's own view,
 * written by the local model or, when meetings are shared, Claude or Codex.
 * Notes Claude or Codex post through Tidy's MCP server are shown too. The
 * notes and their updates belong to the recording, so this panel only
 * shows them.
 */
export function LiveNotesPanel({
  lines,
  onShowLine,
}: {
  lines: LiveLine[];
  onShowLine: (atMs: number) => void;
}) {
  const auto = useUi((s) => s.liveNotesAuto);
  const setAuto = useUi((s) => s.setLiveNotesAuto);
  const views = useUi((s) => s.liveNotesViews);
  const setViews = useUi((s) => s.setLiveNotesViews);
  const custom = useUi((s) => s.liveNotesCustom);
  const setCustom = useUi((s) => s.setLiveNotesCustom);
  const setBy = useUi((s) => s.setLiveNotesBy);
  const { notes, error, busy, by, can, sharing, agentNotes, now, update } = useSharedLiveNotes();
  const asked = views.filter((v) => v !== "custom" || custom.trim());

  const toggle = (id: LiveView) =>
    setViews(views.includes(id) ? views.filter((v) => v !== id) : LIVE_VIEWS.map((v) => v.id).filter((v) => v === id || views.includes(v)));

  const shown = LIVE_VIEWS.filter((v) => views.includes(v.id) && (notes?.views[v.id]?.length ?? 0) > 0);

  return (
    <div className="space-y-3 text-sm">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs">
        <label className="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface py-1 pl-3 pr-1 text-text-muted">
          <Sparkles className="size-3" aria-hidden />
          <span>Written by</span>
          <select
            aria-label="Who writes the live notes"
            value={by}
            onChange={(e) => setBy(e.target.value as LiveNotesBy)}
            className="bg-transparent pr-1 text-text outline-none"
          >
            <option value="local">This Mac</option>
            <option value="claude" disabled={!can("claude")}>
              Claude (Haiku)
            </option>
            <option value="codex" disabled={!can("codex")}>
              Codex (GPT-6 Luna)
            </option>
          </select>
        </label>
        <label className="inline-flex items-center gap-2 text-text-muted">
          <Switch checked={auto} onCheckedChange={setAuto} aria-label="Update the notes by themselves" />
          Update by themselves
        </label>
        <Button size="sm" variant="secondary" disabled={busy || asked.length === 0 || lines.length === 0} onClick={() => void update(true)}>
          {busy ? <Loader2 className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
          Update now
        </Button>
        <span role="status" className="text-text-faint">
          {busy ? "Updating…" : notes ? `Updated ${ago(now - notes.updated_at)} by ${notes.by}` : ""}
        </span>
      </div>

      {!sharing && (
        <p className="text-xs text-text-faint">
          Notes are written on this Mac. To have Claude or Codex write them, turn on sharing in Settings, under Agent connection.
        </p>
      )}

      <div className="flex flex-wrap gap-1.5" role="group" aria-label="What the notes show">
        {LIVE_VIEWS.map((v) => (
          <button
            key={v.id}
            type="button"
            aria-pressed={views.includes(v.id)}
            onClick={() => toggle(v.id)}
            className={`rounded-full border px-2.5 py-1 text-xs transition-colors ${views.includes(v.id) ? "border-brand bg-brand-soft text-text" : "border-border text-text-muted hover:text-text"}`}
          >
            {v.label}
          </button>
        ))}
      </div>
      {views.includes("custom") && (
        <input
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          maxLength={300}
          placeholder="What should your own notes track? For example: risks to the launch"
          aria-label="What your own notes track"
          className="w-full rounded-lg border border-border bg-bg px-3 py-1.5 text-sm outline-none focus:border-brand"
        />
      )}

      {error && (
        <p role="alert" className="text-danger-c">
          {error}
        </p>
      )}

      <div className="scroll-fade max-h-80 space-y-4 overflow-y-auto">
        {shown.length === 0 && agentNotes.length === 0 && (
          <p className="text-text-faint">
            {lines.length === 0
              ? "Notes start once people have spoken."
              : "Notes appear here after a minute or two of conversation, or press Update now."}
          </p>
        )}
        {shown.map((v) => (
          <section key={v.id} aria-label={v.label}>
            <h4 className="text-xs font-semibold text-text">
              {v.id === "custom" && notes?.custom ? `Your own: ${notes.custom}` : v.label}
            </h4>
            <ul className="mt-1 space-y-1.5">
              {notes?.views[v.id]?.map((item, i) => (
                <li key={`${item.text}-${i}`} className="grid grid-cols-[1fr_auto] items-start gap-x-3">
                  <span className="text-text-muted">
                    {item.new && (
                      <span className="mr-1.5 inline-block size-1.5 -translate-y-0.5 rounded-full bg-brand" aria-label="New" />
                    )}
                    {item.text}
                  </span>
                  {item.at_ms != null && (
                    <button
                      type="button"
                      onClick={() => onShowLine(item.at_ms!)}
                      title={`“${item.quote}”`}
                      aria-label={`Show the line at ${clock(item.at_ms)}: ${item.quote}`}
                      className="rounded px-1 text-xs tabular-nums text-text-faint hover:text-brand"
                    >
                      {clock(item.at_ms)}
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </section>
        ))}
        {agentNotes.length > 0 && (
          <section aria-label="From Claude and Codex">
            <h4 className="text-xs font-semibold text-text">From your assistants</h4>
            <ul className="mt-1 space-y-1.5">
              {agentNotes.map((n) => (
                <li key={n.id} className="text-text-muted">
                  <span className="mr-1.5 text-xs text-text-faint">
                    {n.author} · {AGENT_VIEWS[n.view] ?? "Note"}
                  </span>
                  <span className="whitespace-pre-wrap">{n.text}</span>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}
