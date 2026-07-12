import type { LinkInput } from "@/lib/api";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Block = Record<string, any>;

const WIKILINK_RE = /\[\[([^\]]+)\]\]/g;
const TAG_RE = /(?:^|\s)#([A-Za-z0-9_/-]+)/g;

/**
 * Extract wiki-links and #tags from a page's BlockNote blocks. Robust to both
 * a custom `wikilink` inline node (props.pageId/title) and plain-text `[[Title]]`.
 */
export function extractLinksAndTags(blocks: Block[]): {
  links: LinkInput[];
  tags: string[];
} {
  const links: LinkInput[] = [];
  const tags = new Set<string>();
  const seen = new Set<string>();

  const addLink = (dstTitle: string, targetPageId?: string | null) => {
    const title = dstTitle.trim();
    if (!title) return;
    const key = `${targetPageId ?? ""}|${title.toLowerCase()}`;
    if (seen.has(key)) return;
    seen.add(key);
    links.push({ dst_title: title, target_page_id: targetPageId ?? null });
  };

  const scanText = (t: string) => {
    for (const m of t.matchAll(WIKILINK_RE)) addLink(m[1]);
    for (const m of t.matchAll(TAG_RE)) tags.add(m[1]);
  };

  const walk = (content: unknown) => {
    if (typeof content === "string") {
      scanText(content);
    } else if (Array.isArray(content)) {
      for (const item of content) {
        if (typeof item === "string") scanText(item);
        else if (item && typeof item === "object") {
          if (item.type === "wikilink") {
            addLink(item.props?.title ?? "", item.props?.pageId || null);
          } else if (typeof item.text === "string") {
            scanText(item.text);
          }
        }
      }
    }
  };

  for (const b of blocks) walk(b?.content);
  return { links, tags: [...tags] };
}
