import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "light" | "dark" | "system";
export type DbViewKind = "grid" | "board" | "calendar";

/** What's open in the main content pane. */
export type ActivePane =
  | { kind: "page"; pageId: string }
  | { kind: "settings" }
  | { kind: "meeting" }
  | { kind: "graph" }
  | { kind: "planner" }
  | { kind: "home" };

interface UiState {
  // layout
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  setSidebarWidth: (w: number) => void;
  toggleSidebar: () => void;

  // theme
  theme: Theme;
  setTheme: (t: Theme) => void;

  // navigation
  activePane: ActivePane;
  setActivePane: (p: ActivePane) => void;
  openPage: (pageId: string) => void;

  // command palette
  paletteOpen: boolean;
  setPaletteOpen: (open: boolean) => void;

  // ingest ("file a note") dialog
  ingestOpen: boolean;
  setIngestOpen: (open: boolean) => void;

  // onboarding + feature discovery
  onboarded: boolean;
  setOnboarded: (v: boolean) => void;
  dismissedTips: string[];
  dismissTip: (id: string) => void;
  shortcutsOpen: boolean;
  setShortcutsOpen: (open: boolean) => void;

  // tree expansion (page id -> expanded)
  expanded: Record<string, boolean>;
  toggleExpanded: (pageId: string) => void;
  setExpanded: (pageId: string, open: boolean) => void;

  // active database view (database id -> view id)
  dbActiveView: Record<string, string>;
  setDbActiveView: (databaseId: string, viewId: string) => void;

  // meeting: speaker diarization preference
  diarizeEnabled: boolean;
  setDiarizeEnabled: (v: boolean) => void;
}

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 360;
const clampWidth = (w: number) =>
  Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(w)));

export const SIDEBAR_BOUNDS = { min: SIDEBAR_MIN, max: SIDEBAR_MAX };

export const useUi = create<UiState>()(
  persist(
    (set) => ({
      sidebarWidth: 260,
      sidebarCollapsed: false,
      setSidebarWidth: (w) => set({ sidebarWidth: clampWidth(w) }),
      toggleSidebar: () =>
        set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),

      theme: "system",
      setTheme: (theme) => set({ theme }),

      activePane: { kind: "home" },
      setActivePane: (activePane) => set({ activePane }),
      openPage: (pageId) => set({ activePane: { kind: "page", pageId } }),

      paletteOpen: false,
      setPaletteOpen: (paletteOpen) => set({ paletteOpen }),

      ingestOpen: false,
      setIngestOpen: (ingestOpen) => set({ ingestOpen }),

      onboarded: false,
      setOnboarded: (onboarded) => set({ onboarded }),
      dismissedTips: [],
      dismissTip: (id) =>
        set((s) =>
          s.dismissedTips.includes(id)
            ? s
            : { dismissedTips: [...s.dismissedTips, id] },
        ),
      shortcutsOpen: false,
      setShortcutsOpen: (shortcutsOpen) => set({ shortcutsOpen }),

      expanded: {},
      toggleExpanded: (pageId) =>
        set((s) => ({
          expanded: { ...s.expanded, [pageId]: !s.expanded[pageId] },
        })),
      setExpanded: (pageId, open) =>
        set((s) => ({ expanded: { ...s.expanded, [pageId]: open } })),

      dbActiveView: {},
      setDbActiveView: (databaseId, viewId) =>
        set((s) => ({
          dbActiveView: { ...s.dbActiveView, [databaseId]: viewId },
        })),

      diarizeEnabled: true,
      setDiarizeEnabled: (diarizeEnabled) => set({ diarizeEnabled }),
    }),
    {
      name: "appflower-ui",
      // (persist options continue below)
      // Don't persist transient/navigation state.
      partialize: (s) => ({
        sidebarWidth: s.sidebarWidth,
        sidebarCollapsed: s.sidebarCollapsed,
        theme: s.theme,
        expanded: s.expanded,
        dbActiveView: s.dbActiveView,
        diarizeEnabled: s.diarizeEnabled,
        onboarded: s.onboarded,
        dismissedTips: s.dismissedTips,
      }),
    },
  ),
);

// Dev-only: expose the store for browser-driven verification (no effect in prod).
if (import.meta.env.DEV && typeof window !== "undefined") {
  (window as unknown as { __ui?: typeof useUi }).__ui = useUi;
}
