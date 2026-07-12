import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { BlockNoteView } from "@blocknote/mantine";
import { useCreateBlockNote } from "@blocknote/react";
import type { Block, PartialBlock } from "@blocknote/core";
import "@blocknote/core/fonts/inter.css";
import "@blocknote/mantine/style.css";
import "./editor.css";

import { useUi } from "@/store/ui";
import { resolveTheme } from "@/lib/theme";
import { documentsApi, knowledgeApi } from "@/lib/api";
import { extractLinksAndTags } from "@/lib/wikilinks";
import { useDocument } from "@/hooks/useDocument";
import { PageHeader } from "./PageHeader";
import { BacklinksPanel } from "@/components/knowledge/BacklinksPanel";
import { AiAssist } from "@/features/ai/AiAssist";
import { Skeleton } from "@/components/ui/skeleton";
import { FeatureTip } from "@/components/help/FeatureTip";

const Kbd = ({ children }: { children: React.ReactNode }) => (
  <kbd className="rounded border border-border bg-surface px-1 py-0.5 text-2xs font-medium text-text">
    {children}
  </kbd>
);

const SAVE_DEBOUNCE_MS = 600;

function EditorInner({
  pageId,
  initialContent,
}: {
  pageId: string;
  initialContent: PartialBlock[] | undefined;
}) {
  const theme = useUi((s) => s.theme);
  const dark = resolveTheme(theme) === "dark";
  const qc = useQueryClient();
  const editor = useCreateBlockNote({ initialContent });
  const [saving, setSaving] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<string | null>(null);

  const flush = () => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    if (pending.current !== null) {
      const content = pending.current;
      pending.current = null;
      documentsApi
        .update(pageId, content)
        .catch((e) => console.error("autosave failed", e))
        .finally(() => {
          setSaving(false);
          qc.invalidateQueries({ queryKey: ["pages"] });
        });
      // extract wiki-links + tags → knowledge graph (backlinks)
      try {
        const { links, tags } = extractLinksAndTags(editor.document as Block[]);
        knowledgeApi
          .setPageLinks(pageId, links, tags)
          .then(() => qc.invalidateQueries({ queryKey: ["backlinks"] }))
          .catch(() => {});
      } catch {
        /* non-fatal */
      }
    }
  };

  // Save on the editor's change events (debounced); flush on unmount + window blur.
  useEffect(() => {
    const onChange = () => {
      pending.current = JSON.stringify(editor.document as Block[]);
      setSaving(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(flush, SAVE_DEBOUNCE_MS);
    };
    const unsub = editor.onChange(onChange);
    window.addEventListener("blur", flush);
    return () => {
      unsub?.();
      window.removeEventListener("blur", flush);
      flush();
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
        <FeatureTip id="wikilinks">
          Type <Kbd>/</Kbd> for blocks, <Kbd>[[</Kbd> to link another page, or{" "}
          <Kbd>#</Kbd> to tag it.
        </FeatureTip>
        <BlockNoteView
          editor={editor}
          theme={dark ? "dark" : "light"}
          className="appflower-editor"
        />
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

  let initial: PartialBlock[] | undefined;
  try {
    const parsed = JSON.parse(data ?? "[]") as PartialBlock[];
    initial = parsed.length ? parsed : undefined;
  } catch {
    initial = undefined;
  }

  return <EditorInner key={pageId} pageId={pageId} initialContent={initial} />;
}
