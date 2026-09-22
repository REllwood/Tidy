import { Link2, FileText, Tag } from "lucide-react";
import { useUi } from "@/store/ui";
import { useBacklinks, usePageTags } from "@/hooks/usePageLinks";
import type { Backlink, BacklinkKind } from "@/lib/api";

const KIND_LABEL: Partial<Record<BacklinkKind, string>> = {
  task_of: "task",
  meeting_ref: "meeting",
};

/** One row per source page, keeping the first context we were given. */
function dedupe(backlinks: Backlink[]): Backlink[] {
  const bySource = new Map<string, Backlink>();
  for (const link of backlinks) {
    const existing = bySource.get(link.source_page_id);
    if (!existing) bySource.set(link.source_page_id, link);
    else if (!existing.context && link.context)
      bySource.set(link.source_page_id, { ...existing, context: link.context });
  }
  return [...bySource.values()];
}

/** "Linked from" panel + tags, shown beneath the editor. */
export function BacklinksPanel({ pageId }: { pageId: string }) {
  const { data: backlinks } = useBacklinks(pageId);
  const { data: tags } = usePageTags(pageId);
  const openPage = useUi((s) => s.openPage);

  const links = dedupe(backlinks ?? []);
  const hasTags = (tags?.length ?? 0) > 0;
  const hasLinks = links.length > 0;
  if (!hasTags && !hasLinks) return null;

  return (
    <div className="mt-10 border-t border-border pt-5">
      {hasTags && (
        <div className="mb-4 flex flex-wrap items-center gap-1.5">
          <Tag className="size-3.5 text-text-faint" />
          {tags!.map((t) => (
            <span
              key={t}
              className="rounded-full bg-brand-soft px-2 py-0.5 text-xs font-medium text-brand"
            >
              #{t}
            </span>
          ))}
        </div>
      )}
      {hasLinks && (
        <div>
          <div className="mb-2 flex items-center gap-1.5 text-2xs font-semibold uppercase tracking-wide text-text-faint">
            <Link2 className="size-3.5" /> Linked from · {links.length}
          </div>
          <ul className="space-y-1">
            {links.map((b) => (
              <li key={`backlink-${b.source_page_id}`}>
                <button
                  onClick={() => openPage(b.source_page_id)}
                  className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left hover:bg-surface-hover"
                >
                  <span className="mt-0.5 grid size-4 place-items-center text-note">
                    {b.source_icon ?? (
                      <FileText className="size-3.5 text-text-faint" />
                    )}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-1.5">
                      <span className="min-w-0 truncate text-sm font-medium">
                        {b.source_title || "Untitled"}
                      </span>
                      {KIND_LABEL[b.kind] && (
                        <span className="shrink-0 rounded bg-bg-subtle px-1.5 py-0.5 text-2xs font-medium text-text-muted">
                          {KIND_LABEL[b.kind]}
                        </span>
                      )}
                    </span>
                    {b.context && (
                      <span className="block truncate text-xs text-text-faint">
                        {b.context}
                      </span>
                    )}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
