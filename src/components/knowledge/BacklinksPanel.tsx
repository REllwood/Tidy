import { Link2, FileText, Tag } from "lucide-react";
import { useUi } from "@/store/ui";
import { useBacklinks, usePageTags } from "@/hooks/usePageLinks";

/** "Linked from" panel + tags, shown beneath the editor. */
export function BacklinksPanel({ pageId }: { pageId: string }) {
  const { data: backlinks } = useBacklinks(pageId);
  const { data: tags } = usePageTags(pageId);
  const openPage = useUi((s) => s.openPage);

  const hasTags = (tags?.length ?? 0) > 0;
  const hasLinks = (backlinks?.length ?? 0) > 0;
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
            <Link2 className="size-3.5" /> Linked from · {backlinks!.length}
          </div>
          <ul className="space-y-1">
            {backlinks!.map((b) => (
              <li key={b.source_page_id}>
                <button
                  onClick={() => openPage(b.source_page_id)}
                  className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left hover:bg-surface-hover"
                >
                  <span className="mt-0.5 grid size-4 place-items-center text-note">
                    {b.source_icon ?? <FileText className="size-3.5 text-text-faint" />}
                  </span>
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-medium">
                      {b.source_title || "Untitled"}
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
