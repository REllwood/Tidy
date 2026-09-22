import { useEffect, type RefObject } from "react";
import {
  BlockNoteSchema,
  defaultInlineContentSpecs,
  type BlockNoteEditor,
} from "@blocknote/core";
import { SuggestionMenu } from "@blocknote/core/extensions";
import {
  createReactInlineContentSpec,
  SuggestionMenuController,
  type DefaultReactSuggestionItem,
} from "@blocknote/react";
import { FileText, Hash, Plus } from "lucide-react";
import { knowledgeApi, pagesApi, type Page } from "@/lib/api";
import { useCreatePage } from "@/hooks/usePages";
import { useUi } from "@/store/ui";

/** Open a linked page, finding it by title when the link was typed as text. */
async function openLinked(pageId: string, title: string) {
  let id = pageId;
  if (!id) {
    const pages = await pagesApi.list();
    id =
      pages.find((p) => p.title.toLowerCase() === title.toLowerCase())?.id ??
      "";
  }
  if (id) useUi.getState().openPage(id);
}

/**
 * A link to another page, saved as `{type: "wikilink", props: {pageId, title}}`.
 * Backlinks, Markdown export and the vault already read this shape.
 */
export const WikiLink = createReactInlineContentSpec(
  {
    type: "wikilink",
    propSchema: {
      pageId: { default: "" },
      title: { default: "" },
    },
    content: "none",
  },
  {
    render: ({ inlineContent }) => {
      const { pageId, title } = inlineContent.props;
      return (
        <span
          role="link"
          tabIndex={0}
          data-wikilink=""
          title={`Open ${title}`}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void openLinked(pageId, title)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void openLinked(pageId, title);
          }}
          className="cursor-pointer rounded px-0.5 font-medium text-brand underline decoration-brand/40 underline-offset-2 hover:decoration-brand"
        >
          {title || "Untitled"}
        </span>
      );
    },
  },
);

export const editorSchema = BlockNoteSchema.create({
  inlineContentSpecs: { ...defaultInlineContentSpecs, wikilink: WikiLink },
});

export type TidyEditor = BlockNoteEditor<
  typeof editorSchema.blockSchema,
  typeof editorSchema.inlineContentSchema,
  typeof editorSchema.styleSchema
>;

const TRIGGER = "[[";

/**
 * BlockNote only recognises a two-character trigger at the very start of a
 * line, so catch the second "[" before the editor inserts it and open the
 * page picker. Text input rather than key presses: "[" needs Option on some
 * keyboard layouts, and input can arrive without a key press at all.
 */
export function useWikiLinkTrigger(
  editor: TidyEditor,
  container: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const onBeforeInput = (e: InputEvent) => {
      if (e.inputType !== "insertText" || e.data !== "[") return;
      if (!editor.isEditable) return;
      const state = editor.prosemirrorState;
      const { from, empty } = state.selection;
      if (!empty || from < 1) return;
      if (state.doc.textBetween(Math.max(0, from - 1), from) !== "[") return;
      e.preventDefault();
      e.stopPropagation();
      editor.transact((tr) => tr.delete(from - 1, from));
      editor
        .getExtension(SuggestionMenu)
        ?.openSuggestionMenu(TRIGGER, { deleteTriggerCharacter: true });
    };
    element.addEventListener("beforeinput", onBeforeInput, true);
    return () =>
      element.removeEventListener("beforeinput", onBeforeInput, true);
  }, [editor, container]);
}

/** What a #tag may contain (the same rule the backlinks index reads). */
const TAG = /^[A-Za-z0-9_/-]+$/;

/**
 * The tag picker shown after "#": tags already in use, most used first, or
 * a new one. It only opens after a space or at the start of a line, so "C#"
 * stays text, and closes as soon as what's typed can't be a tag, so Enter
 * after "#1 priority" still starts a new line.
 */
export function TagMenu({ editor }: { editor: TidyEditor }) {
  const getItems = async (query: string) => {
    const close = () => editor.getExtension(SuggestionMenu)?.closeMenu();
    if (query && !TAG.test(query)) {
      close();
      return [];
    }
    const q = query.toLowerCase();
    const tags = await knowledgeApi.listTags().catch(() => [] as string[]);
    const matches = tags
      .filter((t) => t.toLowerCase().startsWith(q))
      .concat(tags.filter((t) => !t.toLowerCase().startsWith(q) && t.toLowerCase().includes(q)))
      .slice(0, 8);
    const add = (tag: string) => () =>
      editor.insertInlineContent([`#${tag} `]);
    const items: DefaultReactSuggestionItem[] = matches.map((tag) => ({
      title: `#${tag}`,
      icon: <Hash className="size-4" />,
      onItemClick: add(tag),
    }));
    if (query && !tags.some((t) => t.toLowerCase() === q))
      items.push({
        title: `Add tag #${query}`,
        icon: <Plus className="size-4" />,
        onItemClick: add(query),
      });
    if (!items.length) close();
    return items;
  };
  return (
    <SuggestionMenuController
      triggerCharacter="#"
      getItems={getItems}
      shouldOpen={(tr) => {
        const from = tr.selection.from;
        const before = tr.doc.textBetween(Math.max(0, from - 1), from);
        return before === "" || /\s/.test(before);
      }}
    />
  );
}

/** The page picker shown after "[[". */
export function WikiLinkMenu({
  editor,
  pages,
  currentPageId,
}: {
  editor: TidyEditor;
  pages: Page[] | undefined;
  currentPageId: string;
}) {
  const create = useCreatePage();
  const link = (page: Pick<Page, "id" | "title">) =>
    editor.insertInlineContent([
      {
        type: "wikilink",
        props: { pageId: page.id, title: page.title || "Untitled" },
      },
      " ",
    ]);
  const getItems = async (query: string) => {
    const q = query.trim().toLowerCase();
    const matches = (pages ?? [])
      .filter((p) => p.id !== currentPageId)
      .filter((p) => !q || p.title.toLowerCase().includes(q))
      .sort(
        (a, b) =>
          Number(b.title.toLowerCase().startsWith(q)) -
            Number(a.title.toLowerCase().startsWith(q)) ||
          b.updated_at - a.updated_at,
      )
      .slice(0, 10);
    const items: DefaultReactSuggestionItem[] = matches.map((page) => ({
      title: page.title || "Untitled",
      icon: page.icon ? <span>{page.icon}</span> : <FileText className="size-4" />,
      onItemClick: () => link(page),
    }));
    const exact = matches.some((p) => p.title.toLowerCase() === q);
    if (q && !exact)
      items.push({
        title: `Create “${query.trim()}”`,
        subtext: "New page, linked here",
        icon: <Plus className="size-4" />,
        onItemClick: () => {
          void create
            .mutateAsync({ title: query.trim() })
            .then((page) => link(page))
            .catch(() => undefined);
        },
      });
    return items;
  };
  return (
    <SuggestionMenuController triggerCharacter={TRIGGER} getItems={getItems} />
  );
}
