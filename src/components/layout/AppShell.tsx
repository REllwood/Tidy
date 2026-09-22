import { useCallback, useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useUi, type ActivePane } from "@/store/ui";
import { useThemeEffect } from "@/lib/theme";
import { usePages, usePagesEvents } from "@/hooks/usePages";
import { useLinksEvents } from "@/hooks/usePageLinks";
import { useDatabaseEvents } from "@/hooks/useDatabase";
import type { DatabaseSummary } from "@/lib/api";
import { Sidebar } from "./Sidebar";
import { Splitter } from "./Splitter";
import { TopBar } from "./TopBar";
import { ContentPane } from "./ContentPane";
import { CommandPalette } from "@/components/search/CommandPalette";
import { IngestDialog } from "@/components/ingest/IngestDialog";
import { ShortcutsSheet } from "@/components/help/ShortcutsSheet";
import { ErrorBoundary } from "@/components/ui/error-boundary";
import { errorMessage } from "@/lib/errors";

import { useSharedMeetingFlow } from "@/features/meeting/MeetingFlowProvider";
import { Loader2 } from "lucide-react";

const DEEP_LINK_PANES = [
  "home",
  "planner",
  "graph",
  "meeting",
  "settings",
  "ask",
];

export function AppShell() {
  const { state: meeting } = useSharedMeetingFlow();
  const pane = useUi(s => s.activePane);
  useThemeEffect();
  usePagesEvents();
  useLinksEvents();
  useDatabaseEvents();
  const sidebarWidth = useUi((s) => s.sidebarWidth);
  const collapsed = useUi((s) => s.sidebarCollapsed);
  const toggleSidebar = useUi((s) => s.toggleSidebar);
  const setShortcutsOpen = useUi((s) => s.setShortcutsOpen);
  const setActivePane = useUi((s) => s.setActivePane);
  const setOnboarded = useUi((s) => s.setOnboarded);
  const openPage = useUi((s) => s.openPage);
  const pruneExpanded = useUi((s) => s.pruneExpanded);
  const pruneDbActiveView = useUi((s) => s.pruneDbActiveView);
  const { data: pages, isFetching: pagesFetching } = usePages();
  const qc = useQueryClient();

  // Deep links (bookmarkable views): ?view=home|planner|graph|meeting|settings,
  // ?open=<page title>, ?onboarded=1. Applied once, on the first render where
  // the page list is known (so ?open can be resolved), match or no match.
  const deepLinked = useRef(false);
  useEffect(() => {
    if (deepLinked.current || !pages) return;
    deepLinked.current = true;
    const p = new URLSearchParams(window.location.search);
    if (p.get("onboarded") === "1" && !useUi.getState().onboarded)
      setOnboarded(true);
    const view = p.get("view");
    if (view && DEEP_LINK_PANES.includes(view))
      setActivePane({ kind: view } as ActivePane);
    const open = p.get("open");
    if (open) {
      const pg = pages.find((x) => x.title.toLowerCase() === open.toLowerCase());
      if (pg) openPage(pg.id);
    }
  }, [pages, setActivePane, setOnboarded, openPage]);

  // Safety net: a page can disappear from under the open pane (deleted here,
  // deleted by the MCP server, or gone from a restored session). Never judge
  // mid-refetch, or a page that was just created would look missing.
  useEffect(() => {
    if (!pages || pagesFetching || pane.kind !== "page") return;
    if (!pages.some((p) => p.id === pane.pageId))
      setActivePane({ kind: "home" });
  }, [pages, pagesFetching, pane, setActivePane]);

  // Once the real data is in, drop persisted map entries for things that are
  // gone, so the saved state can't grow forever.
  const pruned = useRef(false);
  useEffect(() => {
    if (pruned.current || !pages) return;
    pruned.current = true;
    pruneExpanded(pages.map((p) => p.id));
    const databases = qc.getQueryData<DatabaseSummary[]>(["databases"]);
    if (databases)
      pruneDbActiveView(databases.map((d) => d.database_id));
  }, [pages, pruneExpanded, pruneDbActiveView, qc]);

  // Global keybindings: ⌘\ sidebar, ⌘K palette, ? or ⌘/ shortcuts sheet.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      const typing =
        !!el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.isContentEditable);
      if ((e.metaKey || e.ctrlKey) && e.key === "\\") {
        e.preventDefault();
        toggleSidebar();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        const ui = useUi.getState();
        // Another dialog owns the keyboard while it's open.
        if (ui.ingestOpen || ui.shortcutsOpen) return;
        e.preventDefault();
        ui.setPaletteOpen(!ui.paletteOpen);
      } else if (
        ((e.metaKey || e.ctrlKey) && e.key === "/") ||
        (e.key === "?" && !typing)
      ) {
        e.preventDefault();
        setShortcutsOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleSidebar, setShortcutsOpen]);

  // The sidebar's width lives in a custom property so a drag can preview it
  // without a store write on every pointer move.
  const shellRef = useRef<HTMLDivElement>(null);
  const previewSidebarWidth = useCallback((width: number) => {
    shellRef.current?.style.setProperty("--sidebar-w", `${width}px`);
  }, []);

  return (
    <div
      ref={shellRef}
      style={{ "--sidebar-w": `${sidebarWidth}px` } as React.CSSProperties}
      className="flex h-full w-full overflow-hidden bg-bg text-text"
    >
      {!collapsed && (
        <>
          <div style={{ width: "var(--sidebar-w)" }} className="h-full shrink-0">
            {/* A broken tree row shouldn't take the whole shell down. */}
            <ErrorBoundary
              fallback={(error, reset) => (
                <SidebarError error={error} reset={reset} />
              )}
            >
              <Sidebar />
            </ErrorBoundary>
          </div>
          <Splitter onPreview={previewSidebarWidth} />
        </>
      )}
      <main className="flex h-full min-w-0 flex-1 flex-col">
        <TopBar />
        {pane.kind !== "meeting" && !["idle", "done", "error"].includes(meeting.phase) && (
          <button onClick={() => setActivePane({ kind: "meeting" })} className="flex shrink-0 items-center gap-2 border-b border-border bg-brand-soft px-4 py-2 text-left text-sm text-brand">
            {meeting.phase !== "recording" && <Loader2 className="size-3.5 animate-spin" />}
            {meeting.phase === "recording"
              ? `Recording meeting · ${Math.floor(meeting.elapsedMs / 1000)} seconds`
              : meeting.phase === "starting"
                ? "Preparing recording…"
                : "Processing meeting…"} · Return to recorder
          </button>
        )}
        <ContentPane />
      </main>
      <CommandPalette />
      <IngestDialog />
      <ShortcutsSheet />
    </div>
  );
}

/** Keeps the shell usable when the sidebar itself fails to render. */
function SidebarError({ error, reset }: { error: Error; reset: () => void }) {
  return (
    <div
      role="alert"
      className="flex h-full flex-col justify-center gap-2 bg-sidebar px-4 text-sm"
    >
      <p className="font-medium">The sidebar couldn't load.</p>
      <p className="text-xs text-text-muted">{errorMessage(error)}</p>
      <button
        onClick={reset}
        className="self-start rounded-md border border-border-strong px-2 py-1 text-xs text-text-muted hover:bg-surface-hover hover:text-text"
      >
        Try again
      </button>
    </div>
  );
}
