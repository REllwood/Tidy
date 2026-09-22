import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { localAi, aiStatusKey, isUserProgress } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";
import { Sparkles, Loader2 } from "lucide-react";
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Editor = any;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type EditorBlock = any;
import { ollamaApi } from "@/lib/api";
import { blocksToMarkdown, markdownToBlocks } from "@/lib/blocksMarkdown";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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

const ACTION_CLASS =
  "rounded-md border border-border bg-surface px-2.5 py-1 text-note text-text-muted hover:bg-surface-hover hover:text-text disabled:opacity-50";

/** Where the result should land, taken before the dialog steals focus. */
interface Target {
  blocks: EditorBlock[] | null;
  text: string;
  anchor: EditorBlock | null;
}

export function AiAssist({ editor }: { editor: Editor }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [result, setResult] = useState("");
  const [action, setAction] = useState<Action | null>(null);
  const [hasSelection, setHasSelection] = useState(false);
  const target = useRef<Target>({ blocks: null, text: "", anchor: null });
  const promptInput = useRef<HTMLInputElement>(null);
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
      // stop showing "Generating locally…" straight away, even if the core
      // cannot be reached to cancel
      generation.current += 1;
      inFlight.current = false;
      setBusy(false);
      const id = request.current;
      request.current = null;
      if (id) await localAi.cancelRequest(id);
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
  // Background meeting processing steps aside for the assistant; only work
  // the person started elsewhere (a download, say) holds it up.
  const unavailable =
    status.isPending ||
    status.isError ||
    !status.data?.ready ||
    status.data.recording_paused ||
    (isUserProgress(status.data.progress) && !busy);
  const [error, setError] = useState<string | null>(null);

  /** Remember what was selected before the dialog took focus. */
  const capture = () => {
    let blocks: EditorBlock[] | null = null;
    let text = "";
    let anchor: EditorBlock | null = null;
    try {
      const selection = editor.getSelection?.();
      blocks = selection?.blocks?.length ? selection.blocks : null;
    } catch {
      blocks = null;
    }
    try {
      text = editor.getSelectedText?.() ?? "";
    } catch {
      text = "";
    }
    try {
      anchor = editor.getTextCursorPosition?.()?.block ?? null;
    } catch {
      anchor = null;
    }
    target.current = { blocks, text, anchor };
    setHasSelection(!!blocks || text.trim().length > 0);
  };

  const docText = (): string => {
    try {
      return blocksToMarkdown(editor.document);
    } catch {
      return "";
    }
  };

  const run = async (next: Action) => {
    if (inFlight.current || unavailable) return;
    const sel = target.current.text;
    if (next === "rewrite" && !hasSelection) return;
    inFlight.current = true;
    const currentGeneration = ++generation.current;
    setAction(next);
    setError(null);
    setResult("");
    setBusy(true);
    try {
      let instruction = prompt.trim();
      let context = "";
      if (next === "summarize") {
        instruction = sel
          ? "Summarise this selection in a few short bullet points, most important first."
          : "Summarise this page: one or two sentences on what it is about, then short bullet points for the key points, decisions and next steps. Leave out anything the page doesn't say.";
        context = sel || docText();
      } else if (next === "rewrite") {
        instruction =
          "Rewrite this text to be clearer and more concise. Keep its meaning, facts and tone. Reply with the rewritten text only.";
        context = sel || docText();
      } else if (next === "continue") {
        instruction =
          "Continue writing naturally from where this leaves off, in the same voice. Reply with the new text only.";
        context = docText().slice(-2000);
      } else if (!instruction) {
        inFlight.current = false;
        setBusy(false);
        promptInput.current?.focus();
        return;
      } else {
        context = sel || docText();
      }
      request.current = crypto.randomUUID();
      const text = await ollamaApi.generate(
        instruction,
        context,
        request.current,
        next,
      );
      if (generation.current === currentGeneration) setResult(text);
    } catch (e) {
      if (generation.current === currentGeneration) setError(errorMessage(e));
    } finally {
      // a cancelled or closed run must never clear the state of a newer one
      if (generation.current === currentGeneration) {
        request.current = null;
        inFlight.current = false;
        setBusy(false);
      }
    }
  };

  const insert = () => {
    if (!result) return;
    const blocks = markdownToBlocks(result);
    const { blocks: selected, anchor } = target.current;
    const doc = editor.document;
    try {
      if (action === "rewrite" && selected?.length) {
        editor.replaceBlocks(selected, blocks);
      } else if (anchor) {
        editor.insertBlocks(blocks, anchor, "after");
      } else if (doc.length === 0) {
        editor.replaceBlocks(doc, blocks);
      } else {
        editor.insertBlocks(blocks, doc[doc.length - 1], "after");
      }
    } catch {
      // the target block has gone; fall back to the end of the page
      const now = editor.document;
      if (now.length === 0) editor.replaceBlocks(now, blocks);
      else editor.insertBlocks(blocks, now[now.length - 1], "after");
    }
    close();
  };

  const close = () => {
    generation.current += 1;
    inFlight.current = false;
    request.current = null;
    setBusy(false);
    cancel.reset();
    setOpen(false);
    setResult("");
    setPrompt("");
    setError(null);
    setAction(null);
  };

  return (
    <>
      <button
        onClick={() => {
          setError(null);
          capture();
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
            {ACTIONS.map((a) => {
              const needsText = !!a.needsSelection && !hasSelection;
              const button = (
                <button
                  onClick={() => run(a.id)}
                  disabled={busy || unavailable || needsText}
                  className={ACTION_CLASS}
                >
                  {a.label}
                </button>
              );
              if (!needsText) return <span key={a.id}>{button}</span>;
              return (
                <Tooltip key={a.id}>
                  <TooltipTrigger
                    render={<span />}
                    tabIndex={0}
                    className="inline-flex rounded-md"
                  >
                    {button}
                  </TooltipTrigger>
                  <TooltipContent>
                    Select some text in the page first.
                  </TooltipContent>
                </Tooltip>
              );
            })}
          </div>

          <input
            ref={promptInput}
            disabled={busy || unavailable}
            aria-label="AI instruction"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && run("ask")}
            placeholder="Ask about this page, or tell it what to do, then press Enter…"
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
              {action === "rewrite" && target.current.blocks
                ? "Replace selection"
                : "Insert"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
