import { Fragment } from "react";
import { PanelLeft, MoreHorizontal, FileDown, Printer } from "lucide-react";
import { useUi } from "@/store/ui";
import { usePages } from "@/hooks/usePages";
import type { Page } from "@/lib/api";
import { exportPageMarkdown, exportPagePdf } from "@/features/export/pageExport";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

function ancestry(pages: Page[], id: string): Page[] {
  const byId = new Map(pages.map((p) => [p.id, p]));
  const chain: Page[] = [];
  let cur = byId.get(id);
  while (cur) {
    chain.unshift(cur);
    cur = cur.parent_id ? byId.get(cur.parent_id) : undefined;
  }
  return chain;
}

export function TopBar() {
  const toggleSidebar = useUi((s) => s.toggleSidebar);
  const openPage = useUi((s) => s.openPage);
  const pane = useUi((s) => s.activePane);
  const { data: pages } = usePages();

  let crumbs: { id?: string; label: string }[];
  if (pane.kind === "page" && pages) {
    crumbs = ancestry(pages, pane.pageId).map((p) => ({
      id: p.id,
      label: (p.icon ? p.icon + " " : "") + (p.title || "Untitled"),
    }));
    if (crumbs.length === 0) crumbs = [{ label: "Page" }];
  } else {
    crumbs = [
      {
        label:
          pane.kind === "settings"
            ? "Settings"
            : pane.kind === "meeting"
              ? "Meeting recorder"
              : pane.kind === "graph"
                ? "Graph view"
                : pane.kind === "planner"
                  ? "Planner"
                  : "Home",
      },
    ];
  }

  return (
    <header
      className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 text-sm text-text-muted"
      data-tauri-drag-region
    >
      <button
        onClick={toggleSidebar}
        aria-label="Toggle sidebar"
        className="grid size-7 place-items-center rounded-md text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
      >
        <PanelLeft className="size-4" />
      </button>
      <nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-1.5">
        {crumbs.map((c, i) => (
          <Fragment key={c.id ?? i}>
            {i > 0 && <span className="text-text-faint">/</span>}
            <button
              disabled={!c.id || i === crumbs.length - 1}
              onClick={() => c.id && openPage(c.id)}
              className={`truncate ${
                i === crumbs.length - 1
                  ? "font-medium text-text"
                  : "hover:text-text"
              }`}
            >
              {c.label}
            </button>
          </Fragment>
        ))}
      </nav>

      {pane.kind === "page" && (
        <DropdownMenu>
          <DropdownMenuTrigger
            aria-label="Page actions"
            className="ml-auto grid size-7 place-items-center rounded-md text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
          >
            <MoreHorizontal className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuItem onSelect={() => exportPageMarkdown(pane.pageId)}>
              <FileDown className="size-4" /> Export as Markdown
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => exportPagePdf()}>
              <Printer className="size-4" /> Export as PDF
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </header>
  );
}
