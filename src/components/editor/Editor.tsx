import { registerPendingEdits } from "@/lib/pendingEdits";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { Loader2 } from "lucide-react";
import { isTauri } from "@/lib/tauri";
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { BlockNoteView } from "@blocknote/mantine";
import { useCreateBlockNote } from "@blocknote/react";
import type { Block, PartialBlock } from "@blocknote/core";
import "@blocknote/core/fonts/inter.css";
import "@blocknote/mantine/style.css";
import "./editor.css";

import { useUi } from "@/store/ui";
import { useResolvedTheme } from "@/lib/theme";
import { documentsApi, knowledgeApi, type LinkInput } from "@/lib/api";
import { extractLinksAndTags } from "@/lib/wikilinks";
import { usePages } from "@/hooks/usePages";
import {
  editorSchema,
  TagMenu,
  WikiLinkMenu,
  useWikiLinkTrigger,
} from "./wikiLinks";
import {
  DOCUMENT_CHANGED_EVENT,
  documentKey,
  invalidateDocument,
  useDocument,
} from "@/hooks/useDocument";
import { PageHeader } from "./PageHeader";
import { BacklinksPanel } from "@/components/knowledge/BacklinksPanel";
import { AiAssist } from "@/features/ai/AiAssist";
import { Skeleton } from "@/components/ui/skeleton";
import { FeatureTip } from "@/components/help/FeatureTip";

import { MeetingRecordingPanel } from "@/features/meeting/MeetingRecordingPanel";
import { MeetingSummaryPanel } from "@/features/meeting/MeetingSummaryPanel";
import {
  stampSeconds,
  stampUnder,
  usePlayback,
} from "@/features/meeting/playback";
import { NoteSummaryStatus } from "@/features/ai/NoteSummaryStatus";

const Kbd = ({ children }: { children: React.ReactNode }) => (
  <kbd className="rounded border border-border bg-surface px-1 py-0.5 text-2xs font-medium text-text">
    {children}
  </kbd>
);

const SAVE_DEBOUNCE_MS = 600;
/**
 * How long a leaving save waits for the previous one. The page list shows
 * `updated_at` ("Edited … ago" and the recents on Home), so saves still
 * refresh it, but at most this often.
 */
const PAGES_REFRESH_MS = 10_000;
/** A closing editor should not queue behind a stalled save for longer. */
const SAVE_HANDOFF_MS = 250;
/** Toast about failing link writes only once it is clearly not a blip. */
const LINK_FAILURES_BEFORE_TOAST = 3;

const wait = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Stable shape for "are these the same links and tags we last sent?". */
function linkSignature(links: LinkInput[], tags: string[]): string {
  return JSON.stringify({
    links: links
      .map((l) =>
        JSON.stringify([
          l.target_page_id ?? "",
          l.dst_title ?? "",
          l.context ?? "",
        ]),
      )
      .sort(),
    tags: [...tags].sort(),
  });
}

function EditorInner({
  pageId,
  initialContent,
  initialRaw,
}: {
  pageId: string;
  initialContent: PartialBlock[] | undefined;
  initialRaw: string;
}) {
  const resolvedTheme = useResolvedTheme();
  const qc = useQueryClient();
  const editor = useCreateBlockNote({
    schema: editorSchema,
    initialContent: initialContent as never,
  });
  const { data: allPages } = usePages();
  const editorArea = useRef<HTMLDivElement>(null);
  useWikiLinkTrigger(editor, editorArea);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [transcribing, setTranscribing] = useState(false);
  /** The stored content we last loaded or wrote; null while unresolved. */
  const [conflict, setConflict] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);
  const lastSave = useRef<Promise<void>>(Promise.resolve());
  const pane = useUi((s) => s.activePane);
  const sourceBlock =
    pane.kind === "page" && pane.pageId === pageId ? pane.blockId : undefined;
  useEffect(() => {
    if (!sourceBlock) return;
    const frame = requestAnimationFrame(() => {
      const element = document.querySelector(
        `[data-id="${CSS.escape(sourceBlock)}"]`,
      );
      element?.scrollIntoView({ block: "center", behavior: "smooth" });
    });
    return () => cancelAnimationFrame(frame);
  }, [sourceBlock, editor]);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** Content has changed since the last write; we serialise only in flush(). */
  const dirty = useRef(false);
  /** What the core holds, so saves can be conditional on it. */
  const baseline = useRef(initialRaw);
  const conflictRef = useRef<string | null>(null);
  conflictRef.current = conflict;
  const suppress = useRef(false);
  const lastPagesRefresh = useRef(0);
  const sentLinks = useRef<string | null>(null);
  const linkFailures = useRef(0);
  const linkToastShown = useRef(false);

  /** Replace the content from outside the editor without scheduling a save. */
  const applyExternal = (blocks: PartialBlock[]) => {
    suppress.current = true;
    try {
      editor.replaceBlocks(editor.document, blocks);
    } finally {
      setTimeout(() => {
        suppress.current = false;
      }, 0);
    }
  };

  const adoptStored = (stored: string) => {
    baseline.current = stored;
    qc.setQueryData(documentKey(pageId), stored);
  };

  const refreshPages = () => {
    const now = Date.now();
    if (now - lastPagesRefresh.current < PAGES_REFRESH_MS) return;
    lastPagesRefresh.current = now;
    void qc.invalidateQueries({ queryKey: ["pages"] });
  };

  /** Push wiki-links and #tags to the knowledge graph when they changed. */
  const syncLinks = (blocks: Block[]) => {
    let links: LinkInput[];
    let tags: string[];
    try {
      ({ links, tags } = extractLinksAndTags(blocks));
    } catch (error) {
      console.error("Could not read this page's links", error);
      return;
    }
    const signature = linkSignature(links, tags);
    if (signature === sentLinks.current) return;
    sentLinks.current = signature;
    knowledgeApi
      .setPageLinks(pageId, links, tags)
      .then(() => {
        linkFailures.current = 0;
        void qc.invalidateQueries({ queryKey: ["backlinks"] });
      })
      .catch((error) => {
        // let the next edit try again
        sentLinks.current = null;
        linkFailures.current += 1;
        console.error("Could not update this page's links", error);
        if (
          linkFailures.current >= LINK_FAILURES_BEFORE_TOAST &&
          !linkToastShown.current
        ) {
          linkToastShown.current = true;
          toast.error("Links and tags on this page are not being updated.");
        }
      });
  };

  /**
   * Write any outstanding change. `leaving` is used by the unmount/blur path,
   * which keeps ordering but will not wait indefinitely on a stalled save.
   */
  const flush = (leaving = false): Promise<void> => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    if (!dirty.current) return lastSave.current;
    const stale = conflictRef.current;
    // While the conflict is unresolved we hold off, except when the editor is
    // going away: typed text cannot be recovered, an external import can.
    if (stale !== null && !leaving) return lastSave.current;
    dirty.current = false;
    const content = JSON.stringify(editor.document as Block[]);
    const expected = stale ?? baseline.current;
    setSaving(true);

    const write = async () => {
      const saved = await documentsApi.updateIfUnchanged(
        pageId,
        expected,
        content,
      );
      if (saved) {
        adoptStored(content);
        refreshPages();
        return;
      }
      const stored = await documentsApi.get(pageId);
      dirty.current = true;
      setConflict(stored);
      setSaving(false);
    };

    // A failed earlier save is already reported below. Allow a newer edit or
    // explicit retry to save, while keeping the promise rejectable for callers.
    const previous = lastSave.current.catch(() => undefined);
    const queue = leaving
      ? Promise.race([previous, wait(SAVE_HANDOFF_MS)])
      : previous;
    lastSave.current = queue.then(write);
    const thisSave = lastSave.current;
    void thisSave.then(
      () => {
        if (lastSave.current !== thisSave) return;
        setSaving(false);
        setSaveError(null);
      },
      (error) => {
        if (lastSave.current !== thisSave) return;
        dirty.current = true;
        setSaving(false);
        setSaveError(errorMessage(error));
      },
    );
    syncLinks(editor.document as Block[]);
    return lastSave.current;
  };

  const flushRef = useRef(flush);
  flushRef.current = flush;
  useEffect(() => registerPendingEdits(() => flushRef.current()), []);

  const keepMine = async () => {
    setResolving(true);
    const content = JSON.stringify(editor.document as Block[]);
    try {
      await documentsApi.update(pageId, content);
      dirty.current = false;
      adoptStored(content);
      refreshPages();
      setConflict(null);
      setSaveError(null);
      setSaving(false);
      syncLinks(editor.document as Block[]);
    } catch (error) {
      setSaveError(errorMessage(error));
    } finally {
      setResolving(false);
    }
  };

  const loadTheirs = () => {
    const stored = conflictRef.current;
    if (stored === null) return;
    let blocks: PartialBlock[] = [];
    try {
      const parsed = JSON.parse(stored) as PartialBlock[];
      blocks = Array.isArray(parsed) && parsed.length ? parsed : [];
    } catch {
      blocks = [];
    }
    applyExternal(blocks.length ? blocks : [{ type: "paragraph" }]);
    dirty.current = false;
    adoptStored(stored);
    sentLinks.current = null;
    setConflict(null);
    setSaveError(null);
    setSaving(false);
  };

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    const listening = listen<{ page_id: string; body_json: string }>(
      "meeting-transcript-updated",
      async ({ payload }) => {
        if (payload.page_id !== pageId || dirty.current) return;
        try {
          await lastSave.current.catch(() => undefined);
          const stored = await documentsApi.get(pageId);
          if (!disposed && !dirty.current && stored === payload.body_json) {
            adoptStored(stored);
            applyExternal(JSON.parse(stored));
          }
        } catch (error) {
          console.error("Could not refresh the saved transcript", error);
        }
      },
    );
    return () => {
      disposed = true;
      void listening
        .then((unlisten) => unlisten())
        .catch((error) => console.error("Transcript listener cleanup", error));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pageId, editor]);

  // The core changed this page behind our back (vault import, MCP sidecar,
  // or a meeting action such as naming speakers).
  useEffect(() => {
    let disposed = false;
    const onChanged = async (changedId: string) => {
        if (changedId !== pageId || disposed) return;
        try {
          if (dirty.current || conflictRef.current !== null) {
            const stored = await documentsApi.get(pageId);
            if (disposed || conflictRef.current !== null) return;
            void invalidateDocument(qc, pageId);
            setConflict(stored);
            setSaving(false);
            return;
          }
          await lastSave.current.catch(() => undefined);
          if (disposed || dirty.current) return;
          const stored = await documentsApi.get(pageId);
          if (disposed || dirty.current) return;
          adoptStored(stored);
          if (JSON.stringify(editor.document as Block[]) === stored) return;
          const parsed = JSON.parse(stored) as PartialBlock[];
          applyExternal(parsed.length ? parsed : [{ type: "paragraph" }]);
          sentLinks.current = null;
        } catch (error) {
          console.error("Could not load the external change", error);
        }
    };
    const onAnnounced = (e: Event) =>
      void onChanged((e as CustomEvent<{ page_id: string }>).detail.page_id);
    window.addEventListener(DOCUMENT_CHANGED_EVENT, onAnnounced);
    const listening = isTauri()
      ? listen<{ page_id: string }>("document-changed", ({ payload }) =>
          onChanged(payload.page_id),
        )
      : null;
    return () => {
      disposed = true;
      window.removeEventListener(DOCUMENT_CHANGED_EVENT, onAnnounced);
      void listening
        ?.then((unlisten) => unlisten())
        .catch((error) => console.error("Document listener cleanup", error));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pageId, editor, qc]);

  // Save on the editor's change events (debounced); flush on unmount + window blur.
  useEffect(() => {
    const onChange = () => {
      if (suppress.current) return;
      dirty.current = true;
      if (conflictRef.current === null) setSaving(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        void flushRef.current().catch(() => undefined);
      }, SAVE_DEBOUNCE_MS);
    };
    const onBlur = () => {
      void flushRef.current().catch(() => undefined);
    };
    const unsub = editor.onChange(onChange);
    window.addEventListener("blur", onBlur);
    return () => {
      unsub?.();
      window.removeEventListener("blur", onBlur);
      void flushRef.current(true).catch(() => undefined);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, pageId]);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-10 pb-48 pt-14">
        <div className="mb-2 flex justify-end">
          <AiAssist editor={editor} />
        </div>
        <PageHeader pageId={pageId} saving={saving} />
        {conflict !== null && (
          <div
            role="alert"
            className="mb-4 rounded-lg border border-border-strong bg-bg-subtle p-3 text-sm"
          >
            <p className="font-medium">This page was changed elsewhere</p>
            <p className="mt-1 text-text-muted">
              Choose which version to keep. Your edits are still in the editor.
            </p>
            <div className="mt-2 flex flex-wrap gap-2">
              <Button
                size="sm"
                disabled={resolving}
                onClick={() => {
                  void keepMine();
                }}
              >
                {resolving && <Loader2 className="size-4 animate-spin" />}
                Keep my version
              </Button>
              <Button
                size="sm"
                variant="secondary"
                disabled={resolving}
                onClick={loadTheirs}
              >
                Load the other version
              </Button>
            </div>
          </div>
        )}
        {saveError && (
          <div
            role="alert"
            className="mb-4 rounded-lg border border-danger-c p-3 text-sm"
          >
            <p>Changes could not be saved: {saveError}</p>
            <Button
              className="mt-2"
              variant="secondary"
              disabled={saving}
              onClick={() => {
                dirty.current = true;
                void flushRef.current().catch(() => undefined);
              }}
            >
              {saving && <Loader2 className="size-4 animate-spin" />}Retry save
            </Button>
          </div>
        )}
        {/* The notes come first. The recording's tools fold away below them
            once a transcript is saved. */}
        <MeetingSummaryPanel pageId={pageId} />
        <MeetingRecordingPanel
          pageId={pageId}
          beforeTranscribe={() => flushRef.current()}
          onBusy={setTranscribing}
          onResult={(result) => {
            if (result.applied) {
              adoptStored(result.body_json);
              applyExternal(JSON.parse(result.body_json));
              sentLinks.current = null;
            }
          }}
        />
        <NoteSummaryStatus pageId={pageId} />
        <FeatureTip id="wikilinks">
          Type <Kbd>/</Kbd> for blocks, <Kbd>[[</Kbd> to link another page, or{" "}
          <Kbd>#</Kbd> to tag it.
        </FeatureTip>
        <div
          ref={editorArea}
          onClick={(e) => {
            // Clicking a transcript line's stamp plays the meeting from there.
            const stamp = stampUnder(e.clientX, e.clientY);
            const seconds = stamp ? stampSeconds(stamp) : null;
            if (seconds !== null) usePlayback.getState().play(pageId, seconds);
          }}
        >
          <BlockNoteView
            editor={editor}
            editable={!transcribing}
            theme={resolvedTheme}
            className="tidy-editor"
          >
            <WikiLinkMenu
              editor={editor}
              pages={allPages}
              currentPageId={pageId}
            />
            <TagMenu editor={editor} />
          </BlockNoteView>
        </div>
        <BacklinksPanel pageId={pageId} />
      </div>
    </div>
  );
}

/** Loads the stored document, then mounts the editor (remounts per page). */
export function DocumentEditor({ pageId }: { pageId: string }) {
  const { data, isLoading, isError } = useDocument(pageId);

  if (isLoading) {
    return (
      <div className="h-full overflow-y-auto">
        <div className="mx-auto max-w-3xl px-10 pt-14">
          <Skeleton className="mb-3 size-[60px] rounded-lg" />
          <Skeleton className="mb-6 h-10 w-2/3" />
          <Skeleton className="mb-2 h-4 w-full" />
          <Skeleton className="mb-2 h-4 w-11/12" />
          <Skeleton className="h-4 w-3/4" />
        </div>
      </div>
    );
  }
  if (isError) {
    return (
      <div className="mx-auto max-w-3xl px-10 pt-14 text-danger-c">
        Couldn't load this page.
      </div>
    );
  }

  const raw = data ?? "[]";
  let initial: PartialBlock[] | undefined;
  try {
    const parsed = JSON.parse(raw) as PartialBlock[];
    initial = parsed.length ? parsed : undefined;
  } catch {
    initial = undefined;
  }

  return (
    <EditorInner
      key={pageId}
      pageId={pageId}
      initialContent={initial}
      initialRaw={raw}
    />
  );
}
