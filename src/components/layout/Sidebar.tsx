import {
  Search,
  Mic,
  LayoutGrid,
  Plus,
  Settings,
  Sun,
  Moon,
  Star,
  FileUp,
  Waypoints,
  Sparkles,
  CalendarCheck,
} from "lucide-react";
import { useUi } from "@/store/ui";
import { resolveTheme } from "@/lib/theme";
import { usePagesTree, useCreatePage } from "@/hooks/usePages";
import { PageTree } from "@/components/sidebar/PageTree";
import { pickAndImportMarkdown } from "@/features/export/pageExport";

function NavButton({
  icon,
  label,
  active,
  kbd,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  kbd?: string;
  onClick?: () => void;
}) {
  return (
    <button
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={`group flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm transition-colors ${
        active
          ? "bg-brand-soft font-medium text-text"
          : "text-text-muted hover:bg-surface-hover hover:text-text"
      }`}
    >
      <span
        className={`flex w-4 justify-center transition-colors ${
          active ? "text-brand" : "text-text-muted group-hover:text-text"
        }`}
      >
        {icon}
      </span>
      <span className="flex-1 truncate">{label}</span>
      {kbd && (
        <kbd className="rounded border border-border bg-surface px-1.5 text-2xs text-text-faint">
          {kbd}
        </kbd>
      )}
    </button>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-2 pb-1 pt-3.5 text-2xs font-semibold uppercase tracking-wide text-text-faint">
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

  const { tree, data: pages } = usePagesTree();
  const favorites = (pages ?? []).filter((p) => p.is_favorite);
  const create = useCreatePage();

  return (
    <aside
      aria-label="Workspace navigation"
      className="flex h-full flex-col overflow-hidden border-r border-border bg-sidebar"
    >
      <div className="flex h-12 items-center gap-2 px-3" data-tauri-drag-region>
        <img src="/tidy-icon.svg" alt="" className="size-6" draggable={false} />
        <span className="text-sm font-semibold">Tidy</span>
      </div>

      <div className="flex-1 overflow-y-auto px-2 pb-2">
        <NavButton
          icon={<Search className="size-4" />}
          label="Search"
          kbd="⌘K"
          onClick={() => setPaletteOpen(true)}
        />
        <NavButton
          icon={<Sparkles className="size-4" />}
          label="File a note"
          onClick={() => setIngestOpen(true)}
        />
        <NavButton
          icon={<Mic className="size-4" />}
          label="New meeting"
          active={pane.kind === "meeting"}
          onClick={() => setActivePane({ kind: "meeting" })}
        />
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
          icon={<Waypoints className="size-4" />}
          label="Graph view"
          active={pane.kind === "graph"}
          onClick={() => setActivePane({ kind: "graph" })}
        />

        {favorites.length > 0 && (
          <>
            <SectionLabel>Favorites</SectionLabel>
            {favorites.map((p) => (
              <button
                key={p.id}
                onClick={() => openPage(p.id)}
                className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-sm text-text-muted transition-colors hover:bg-surface-hover hover:text-text"
              >
                <span className="grid size-4 place-items-center text-md">
                  {p.icon ?? <Star className="size-3.5 text-text-faint" />}
                </span>
                <span className="truncate">{p.title || "Untitled"}</span>
              </button>
            ))}
          </>
        )}

        <SectionLabel>Workspace</SectionLabel>
        <PageTree tree={tree} />
      </div>

      <div className="border-t border-border px-2 py-2">
        <NavButton
          icon={<Plus className="size-4" />}
          label="New page"
          onClick={() =>
            create.mutate(
              { title: "Untitled" },
              { onSuccess: (p) => openPage(p.id) },
            )
          }
        />
        <NavButton
          icon={<FileUp className="size-4" />}
          label="Import Markdown"
          onClick={async () => {
            const id = await pickAndImportMarkdown();
            if (id) openPage(id);
          }}
        />
        <NavButton
          icon={<Settings className="size-4" />}
          label="Settings"
          active={pane.kind === "settings"}
          onClick={() => setActivePane({ kind: "settings" })}
        />
        <NavButton
          icon={isDark ? <Sun className="size-4" /> : <Moon className="size-4" />}
          label={isDark ? "Light mode" : "Dark mode"}
          onClick={() => setTheme(isDark ? "light" : "dark")}
        />
      </div>
    </aside>
  );
}
