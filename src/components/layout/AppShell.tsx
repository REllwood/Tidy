import { useEffect, useRef } from "react";
import { useUi, type ActivePane } from "@/store/ui";
import { useThemeEffect } from "@/lib/theme";
import { usePages, usePagesEvents } from "@/hooks/usePages";
import { useLinksEvents } from "@/hooks/usePageLinks";
import { Sidebar } from "./Sidebar";
import { Splitter } from "./Splitter";
import { TopBar } from "./TopBar";
import { ContentPane } from "./ContentPane";
import { CommandPalette } from "@/components/search/CommandPalette";
import { IngestDialog } from "@/components/ingest/IngestDialog";
import { ShortcutsSheet } from "@/components/help/ShortcutsSheet";

export function AppShell() {
  useThemeEffect();
  usePagesEvents();
  useLinksEvents();
  const sidebarWidth = useUi((s) => s.sidebarWidth);
  const collapsed = useUi((s) => s.sidebarCollapsed);
  const toggleSidebar = useUi((s) => s.toggleSidebar);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const setShortcutsOpen = useUi((s) => s.setShortcutsOpen);
  const setActivePane = useUi((s) => s.setActivePane);
  const setOnboarded = useUi((s) => s.setOnboarded);
  const openPage = useUi((s) => s.openPage);
  const { data: pages } = usePages();

  // Deep links (bookmarkable views): ?view=home|planner|graph|meeting|settings,
  // ?open=<page title>, ?onboarded=1. Applied once on first load.
  const deepLinked = useRef(false);
  useEffect(() => {
    if (deepLinked.current) return;
    const p = new URLSearchParams(window.location.search);
    if (p.get("onboarded") === "1") setOnboarded(true);
    const view = p.get("view");
    const panes = ["home", "planner", "graph", "meeting", "settings"];
    if (view && panes.includes(view)) {
      setActivePane({ kind: view } as ActivePane);
      deepLinked.current = true;
    }
    const open = p.get("open");
    if (open && pages) {
      const pg = pages.find((x) => x.title.toLowerCase() === open.toLowerCase());
      if (pg) {
        openPage(pg.id);
        deepLinked.current = true;
      }
    }
  }, [pages, setActivePane, setOnboarded, openPage]);

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
        e.preventDefault();
        setPaletteOpen(true);
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
  }, [toggleSidebar, setPaletteOpen, setShortcutsOpen]);

  return (
    <div className="flex h-full w-full overflow-hidden bg-bg text-text">
      {!collapsed && (
        <>
          <div style={{ width: sidebarWidth }} className="h-full shrink-0">
            <Sidebar />
          </div>
          <Splitter />
        </>
      )}
      <main className="flex h-full min-w-0 flex-1 flex-col">
        <TopBar />
        <ContentPane />
      </main>
      <CommandPalette />
      <IngestDialog />
      <ShortcutsSheet />
    </div>
  );
}
