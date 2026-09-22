import type { LinkInput } from "@/lib/api";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Block = Record<string, any>;

// No brackets inside: an unfinished "[[" earlier in the line (a picker that
// was dismissed) must not swallow the link after it.
const WIKILINK_RE = /\[\[([^[\]]+)\]\]/g;
const TAG_RE = /(?:^|\s)#([A-Za-z0-9_/-]+)/g;
/** Roughly how much plain text to keep either side of a link for its preview. */
const CONTEXT_CHARS = 80;

interface Flattened {
  /** Every inline run of the block joined into one plain string. */
  text: string;
  /** Offset of a rendered `[[Title]]` → the page id it already resolves to. */
  resolved: Map<number, string | null>;
}

/**
 * Join a block's inline content into a single string so a `[[link]]` split
 * across formatting runs still matches, and remember where the custom
 * `wikilink` nodes landed so their resolved page id survives the scan.
 */
function flatten(content: unknown): Flattened {
  let text = "";
  const resolved = new Map<number, string | null>();

  const push = (run: unknown) => {
    if (typeof run === "string") {
      text += run;
      return;
    }
    if (!run || typeof run !== "object") return;
    const node = run as Block;
    if (node.type === "wikilink") {
      const title = String(node.props?.title ?? "").trim();
      if (!title) return;
      resolved.set(text.length, node.props?.pageId || null);
      text += `[[${title}]]`;
      return;
    }
    if (typeof node.text === "string") {
      text += node.text;
      return;
    }
    // link runs (and anything else nesting inline content)
    if (Array.isArray(node.content)) for (const child of node.content) push(child);
  };

  if (typeof content === "string") text = content;
  else if (Array.isArray(content)) for (const item of content) push(item);
  return { text, resolved };
}

/** A tidy one-line snippet of the text around a link, for the backlinks list. */
function contextAround(text: string, start: number, end: number): string | null {
  const pad = Math.max(0, Math.round((CONTEXT_CHARS - (end - start)) / 2));
  const from = Math.max(0, start - pad);
  const to = Math.min(text.length, end + pad);
  let snippet = text.slice(from, to).replace(/\s+/g, " ").trim();
  if (!snippet) return null;
  if (from > 0) snippet = `…${snippet}`;
  if (to < text.length) snippet = `${snippet}…`;
  return snippet;
}

/** True for the `tableContent` shape BlockNote stores on `table` blocks. */
function isTableContent(content: unknown): content is { rows: Block[] } {
  return (
    !!content &&
    typeof content === "object" &&
    !Array.isArray(content) &&
    Array.isArray((content as Block).rows)
  );
}

/**
 * Extract wiki-links and #tags from a page's BlockNote blocks. Robust to both
 * a custom `wikilink` inline node (props.pageId/title) and plain-text
 * `[[Title]]`, and walks nested blocks and table cells.
 */
export function extractLinksAndTags(blocks: Block[]): {
  links: LinkInput[];
  tags: string[];
} {
  const links: LinkInput[] = [];
  const tags = new Set<string>();
  const seen = new Set<string>();

  const addLink = (
    dstTitle: string,
    targetPageId: string | null,
    context: string | null,
  ) => {
    const title = dstTitle.trim();
    if (!title) return;
    const key = `${targetPageId ?? ""}|${title.toLowerCase()}`;
    if (seen.has(key)) return;
    seen.add(key);
    links.push({
      dst_title: title,
      target_page_id: targetPageId,
      context,
    });
  };

  const scan = (content: unknown) => {
    const { text, resolved } = flatten(content);
    if (!text) return;
    for (const match of text.matchAll(WIKILINK_RE)) {
      const start = match.index ?? 0;
      addLink(
        match[1],
        resolved.get(start) ?? null,
        contextAround(text, start, start + match[0].length),
      );
    }
    for (const match of text.matchAll(TAG_RE)) tags.add(match[1]);
  };

  const walk = (block: unknown) => {
    if (!block || typeof block !== "object") return;
    const node = block as Block;
    const content = node.content;
    if (isTableContent(content)) {
      for (const row of content.rows) {
        for (const cell of row?.cells ?? [])
          scan(Array.isArray(cell) ? cell : cell?.content);
      }
    } else {
      scan(content);
    }
    for (const child of node.children ?? []) walk(child);
  };

  for (const block of blocks) walk(block);
  return { links, tags: [...tags] };
}
