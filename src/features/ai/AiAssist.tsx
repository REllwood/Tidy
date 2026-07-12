import { useState } from "react";
import { Sparkles, Loader2 } from "lucide-react";
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Editor = any;
import { ollamaApi } from "@/lib/api";
import { blocksToMarkdown, markdownToBlocks } from "@/lib/blocksMarkdown";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

type Action = "ask" | "summarize" | "rewrite" | "continue";

const ACTIONS: { id: Action; label: string; needsSelection?: boolean }[] = [
  { id: "ask", label: "Ask…" },
  { id: "summarize", label: "Summarise page" },
  { id: "rewrite", label: "Rewrite selection", needsSelection: true },
  { id: "continue", label: "Continue writing" },
];

export function AiAssist({ editor }: { editor: Editor }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [result, setResult] = useState("");
  const [error, setError] = useState<string | null>(null);

  const selectedText = (): string => {
    try {
      return editor.getSelectedText?.() ?? "";
    } catch {
      return "";
    }
  };
  const docText = (): string => {
    try {
      return blocksToMarkdown(editor.document);
    } catch {
      return "";
    }
  };

  const run = async (action: Action) => {
    setError(null);
    setResult("");
    setBusy(true);
    try {
      const sel = selectedText();
      let instruction = prompt.trim();
      let context = "";
      if (action === "summarize") {
        instruction = "Summarize the following notes in a short paragraph.";
        context = sel || docText();
      } else if (action === "rewrite") {
        instruction = "Rewrite the following text to be clearer and more concise.";
        context = sel || docText();
      } else if (action === "continue") {
        instruction = "Continue writing naturally from where this leaves off.";
        context = docText().slice(-2000);
      } else if (!instruction) {
        setBusy(false);
        return;
      }
      const text = await ollamaApi.generate(instruction, context);
      // simple typewriter reveal
      setResult("");
      for (let i = 1; i <= text.length; i += Math.max(1, Math.round(text.length / 60))) {
        setResult(text.slice(0, i));
        // eslint-disable-next-line no-await-in-loop
        await new Promise((r) => setTimeout(r, 12));
      }
      setResult(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const insert = () => {
    if (!result) return;
    const blocks = markdownToBlocks(result);
    const doc = editor.document;
    if (doc.length === 0) {
      editor.replaceBlocks(doc, blocks);
    } else {
      editor.insertBlocks(blocks, doc[doc.length - 1], "after");
    }
    close();
  };

  const close = () => {
    setOpen(false);
    setResult("");
    setPrompt("");
    setError(null);
  };

  return (
    <>
      <button
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-note text-brand transition-colors hover:bg-surface-hover"
      >
        <Sparkles className="size-3.5" /> Ask AI
      </button>

      <Dialog open={open} onOpenChange={(o) => (o ? setOpen(true) : close())}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Sparkles className="size-4 text-brand" /> AI assistant
            </DialogTitle>
          </DialogHeader>

          <div className="flex flex-wrap gap-1.5">
            {ACTIONS.map((a) => (
              <button
                key={a.id}
                onClick={() => run(a.id)}
                disabled={busy}
                className="rounded-md border border-border bg-surface px-2.5 py-1 text-note text-text-muted hover:bg-surface-hover hover:text-text disabled:opacity-50"
              >
                {a.label}
              </button>
            ))}
          </div>

          <input
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && run("ask")}
            placeholder="Or type an instruction and press Enter…"
            className="w-full rounded-md border border-border bg-surface px-3 py-2 text-sm outline-none"
          />

          {busy && (
            <div className="flex items-center gap-2 text-sm text-text-muted">
              <Loader2 className="size-4 animate-spin" /> Generating locally…
            </div>
          )}
          {error && <div className="text-sm text-danger-c">{error}</div>}
          {result && (
            <div className="max-h-60 overflow-y-auto whitespace-pre-wrap rounded-lg border border-border bg-bg-subtle p-3 text-sm">
              {result}
            </div>
          )}

          <DialogFooter>
            <Button variant="secondary" onClick={close}>
              Cancel
            </Button>
            <Button onClick={insert} disabled={!result || busy}>
              Insert
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
