import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { localAi, aiStatusKey } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";
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
  const inFlight = useRef(false);
  const request = useRef<string | null>(null);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
      if (request.current)
        localAi
          .cancelRequest(request.current)
          .catch((error) =>
            console.warn("Could not cancel AI request", errorMessage(error)),
          );
    },
    [],
  );
  const cancel = useMutation({
    mutationFn: async () => {
      generation.current += 1;
      if (request.current) await localAi.cancelRequest(request.current);
    },
    onSuccess: () => close(),
  });
  const status = useQuery({
    queryKey: aiStatusKey,
    queryFn: localAi.status,
    enabled: open,
    refetchInterval: open ? 1500 : false,
  });
  const setPane = useUi((s) => s.setActivePane);
  const unavailable =
    status.isPending ||
    status.isError ||
    !status.data?.ready ||
    status.data.recording_paused ||
    (!!status.data.progress && !busy);
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
    if (inFlight.current || unavailable) return;
    inFlight.current = true;
    const currentGeneration = ++generation.current;
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
        instruction =
          "Rewrite the following text to be clearer and more concise.";
        context = sel || docText();
      } else if (action === "continue") {
        instruction = "Continue writing naturally from where this leaves off.";
        context = docText().slice(-2000);
      } else if (!instruction) {
        setBusy(false);
        return;
      } else {
        context = sel || docText();
      }
      request.current = crypto.randomUUID();
      const text = await ollamaApi.generate(
        instruction,
        context,
        request.current,
      );
      if (generation.current === currentGeneration) setResult(text);
    } catch (e) {
      if (generation.current === currentGeneration) setError(errorMessage(e));
    } finally {
      request.current = null;
      inFlight.current = false;
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
    generation.current += 1;
    cancel.reset();
    setOpen(false);
    setResult("");
    setPrompt("");
    setError(null);
  };

  return (
    <>
      <button
        onClick={() => {
          setError(null);
          setOpen(true);
        }}
        className="inline-flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-note text-brand transition-colors hover:bg-surface-hover"
      >
        <Sparkles className="size-3.5" /> Ask AI
      </button>

      <Dialog
        open={open}
        onOpenChange={(o) => {
          if (!busy) o ? setOpen(true) : close();
        }}
      >
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Sparkles className="size-4 text-brand" /> AI assistant
            </DialogTitle>
          </DialogHeader>

          {status.isPending ? (
            <p
              role="status"
              className="flex items-center gap-2 text-sm text-text-muted"
            >
              <Loader2 className="size-4 animate-spin" />
              Checking local AI…
            </p>
          ) : status.error ? (
            <p role="alert" className="text-sm text-danger-c">
              {errorMessage(status.error)}
            </p>
          ) : status.data?.recording_paused ? (
            <p className="rounded-lg bg-brand-soft p-3 text-sm">
              AI assistance will be available after the recording has finished
              processing.
            </p>
          ) : !status.data?.ready ? (
            <div className="rounded-lg bg-bg-subtle p-3 text-sm">
              <p>
                Enable local AI and finish downloading an answer model to use
                the assistant.
              </p>
              <Button
                className="mt-3"
                size="sm"
                onClick={() => {
                  close();
                  setPane({ kind: "settings" });
                }}
              >
                Open AI settings
              </Button>
            </div>
          ) : status.data.progress && !busy ? (
            <p
              role="status"
              className="flex items-center gap-2 text-sm text-text-muted"
            >
              <Loader2 className="size-4 animate-spin" />
              {status.data.progress.label}. The assistant will be ready when
              this finishes.
            </p>
          ) : null}
          <div className="flex flex-wrap gap-1.5">
            {ACTIONS.map((a) => (
              <button
                key={a.id}
                onClick={() => run(a.id)}
                disabled={busy || unavailable}
                className="rounded-md border border-border bg-surface px-2.5 py-1 text-note text-text-muted hover:bg-surface-hover hover:text-text disabled:opacity-50"
              >
                {a.label}
              </button>
            ))}
          </div>

          <input
            disabled={busy || unavailable}
            aria-label="AI instruction"
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
          {cancel.error && (
            <p role="alert" className="text-sm text-danger-c">
              {errorMessage(cancel.error)}
            </p>
          )}
          {error && (
            <div role="alert" className="text-sm text-danger-c">
              {error}
            </div>
          )}
          {result && (
            <div className="max-h-60 overflow-y-auto whitespace-pre-wrap rounded-lg border border-border bg-bg-subtle p-3 text-sm">
              {result}
            </div>
          )}

          <DialogFooter>
            <Button
              variant="secondary"
              disabled={cancel.isPending}
              onClick={() => (busy ? cancel.mutate() : close())}
            >
              {cancel.isPending && <Loader2 className="size-3 animate-spin" />}
              {busy ? "Cancel generation" : "Close"}
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
