import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Search,
  MessageSquare,
  Mic,
  LayoutGrid,
  Plus,
  Settings,
  Sun,
  Moon,
  Star,
  FileUp,
  Waypoints,
  FileText,
  CalendarCheck,
  Loader2,
} from "lucide-react";
import { useUi } from "@/store/ui";
import { resolveTheme } from "@/lib/theme";
import { usePagesTree, useCreatePage } from "@/hooks/usePages";
import { PageTree } from "@/components/sidebar/PageTree";
import { pickAndImportMarkdown } from "@/features/export/pageExport";
import { Brand } from "@/components/brand/Brand";

function NavButton({
  icon,
  label,
  active,
  kbd,
  onClick,
  busy = false,
}: {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  kbd?: string;
  onClick?: () => void;
  busy?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={busy}
      aria-busy={busy}
      aria-current={active ? "page" : undefined}
      className={`group relative flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-note transition-colors disabled:opacity-60 ${active ? "bg-brand-soft font-semibold text-text" : "text-text-muted hover:bg-surface-hover hover:text-text"}`}
    >
      <span
        className={
          active ? "text-brand" : "text-text-faint group-hover:text-text"
        }
      >
        {busy ? <Loader2 className="size-4 animate-spin" /> : icon}
      </span>
      <span className="flex-1 truncate">{label}</span>
      {kbd && (
        <kbd className="rounded border border-border-strong/60 px-1 text-2xs text-text-faint">
          {kbd}
        </kbd>
      )}
      {active && <span aria-hidden className="size-1 rounded-full bg-brand" />}
    </button>
  );
}
function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-3 pb-2 pt-6 text-[10px] font-semibold uppercase tracking-[0.16em] text-text-faint">
      {children}
    </div>
  );
}
export function Sidebar() {
  const setActivePane = useUi((s) => s.setActivePane);
  const openPage = useUi((s) => s.openPage);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const setIngestOpen = useUi((s) => s.setIngestOpen);
  const pane = useUi((s) => s.activePane);
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const isDark = resolveTheme(theme) === "dark";
  const { tree, data: pages, isPending, error } = usePagesTree();
  const favourites = (pages ?? []).filter((p) => p.is_favorite);
  const create = useCreatePage();
  const qc = useQueryClient();
  const importNote = useMutation({
    mutationFn: pickAndImportMarkdown,
    onSuccess: async (id) => {
      if (id) {
        await qc.invalidateQueries({ queryKey: ["pages"] });
        openPage(id);
      }
    },
  });
  return (
    <aside
      aria-label="Workspace navigation"
      className="flex h-full flex-col overflow-hidden bg-sidebar"
    >
      <div
        className="flex h-[88px] shrink-0 items-center justify-between px-4"
        data-tauri-drag-region
      >
        <Brand />
        <span className="text-[10px] font-medium uppercase tracking-widest text-text-faint">
          Workspace
        </span>
      </div>
      <div className="px-3">
        <button
          onClick={() => setPaletteOpen(true)}
          className="flex w-full items-center gap-2 rounded-lg border border-border-strong/60 bg-surface/60 px-3 py-2.5 text-note text-text-muted hover:border-brand/50"
        >
          <Search className="size-3.5" />
          <span className="flex-1 text-left">Search anything</span>
          <kbd className="text-2xs text-text-faint">⌘K</kbd>
        </button>
      </div>
      <div className="flex-1 overflow-y-auto px-3 pb-4">
        <SectionLabel>Your space</SectionLabel>
        <nav aria-label="Main navigation" className="space-y-1">
          <NavButton
            icon={<LayoutGrid className="size-4" />}
            label="Home"
            active={pane.kind === "home"}
            onClick={() => setActivePane({ kind: "home" })}
          />
          <NavButton
            icon={<CalendarCheck className="size-4" />}
            label="Planner"
            active={pane.kind === "planner"}
            onClick={() => setActivePane({ kind: "planner" })}
          />
          <NavButton
            icon={<MessageSquare className="size-4" />}
            label="Ask meetings"
            active={pane.kind === "ask"}
            onClick={() => setActivePane({ kind: "ask" })}
          />
          <NavButton icon={<Mic className="size-4" />} label="Recording history" active={pane.kind === "history"} onClick={() => setActivePane({kind: "history"})} />
          <NavButton
            icon={<Waypoints className="size-4" />}
            label="Graph view"
            active={pane.kind === "graph"}
            onClick={() => setActivePane({ kind: "graph" })}
          />
        </nav>
        <SectionLabel>Capture</SectionLabel>
        <NavButton
          icon={<Mic className="size-4" />}
          label="New meeting"
          active={pane.kind === "meeting"}
          onClick={() => setActivePane({ kind: "meeting" })}
        />
        <NavButton
          icon={<FileText className="size-4" />}
          label="File a note"
          onClick={() => setIngestOpen(true)}
        />
        {favourites.length > 0 && (
          <>
            <SectionLabel>Favourites</SectionLabel>
            {favourites.map((p) => (
              <button
                key={p.id}
                onClick={() => openPage(p.id)}
                className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-note text-text-muted hover:bg-surface-hover"
              >
                <span className="grid size-4 shrink-0 place-items-center">
                  {p.icon ?? <Star className="size-3.5" />}
                </span>
                <span className="truncate">{p.title || "Untitled"}</span>
              </button>
            ))}
          </>
        )}
        <SectionLabel>Pages</SectionLabel>
        {isPending ? (
          <p
            role="status"
            className="flex items-center gap-2 px-3 text-xs text-text-muted"
          >
            <Loader2 className="size-3 animate-spin" />
            Loading pages…
          </p>
        ) : (
          <PageTree tree={tree} />
        )}
        {error && (
          <p role="alert" className="px-3 text-xs text-danger-c">
            Couldn't load pages. {String(error)}
          </p>
        )}
        <NavButton
          icon={<Plus className="size-4" />}
          label={create.isPending ? "Creating page…" : "New page"}
          busy={create.isPending}
          onClick={() =>
            create.mutate(
              { title: "Untitled" },
              { onSuccess: (p) => openPage(p.id) },
            )
          }
        />
        {create.error && (
          <p role="alert" className="px-3 text-xs text-danger-c">
            {String(create.error)}
          </p>
        )}
      </div>
      <div className="shrink-0 border-t border-border px-3 py-3">
        <NavButton
          icon={<FileUp className="size-4" />}
          label={importNote.isPending ? "Importing note…" : "Import Markdown"}
          busy={importNote.isPending}
          onClick={() => importNote.mutate()}
        />
        {importNote.error && (
          <p role="alert" className="px-3 text-xs text-danger-c">
            {String(importNote.error)}
          </p>
        )}
        <div className="flex items-center gap-1">
          <NavButton
            icon={<Settings className="size-4" />}
            label="Settings"
            active={pane.kind === "settings"}
            onClick={() => setActivePane({ kind: "settings" })}
          />
          <button
            aria-label={isDark ? "Light mode" : "Dark mode"}
            title={isDark ? "Light mode" : "Dark mode"}
            onClick={() => setTheme(isDark ? "light" : "dark")}
            className="grid size-8 shrink-0 place-items-center rounded-lg text-text-faint hover:bg-surface-hover hover:text-text"
          >
            {isDark ? <Sun className="size-4" /> : <Moon className="size-4" />}
          </button>
        </div>
      </div>
    </aside>
  );
}
