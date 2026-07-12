import { lazy, Suspense } from "react";
import { useUi } from "@/store/ui";

// Lazy-load heavy panes so the initial bundle stays small.
const PageRouter = lazy(() =>
  import("@/components/PageRouter").then((m) => ({ default: m.PageRouter })),
);
const MeetingRecorder = lazy(() =>
  import("@/features/meeting/MeetingRecorder").then((m) => ({
    default: m.MeetingRecorder,
  })),
);
const SettingsView = lazy(() =>
  import("@/features/settings/SettingsView").then((m) => ({
    default: m.SettingsView,
  })),
);
const GraphView = lazy(() =>
  import("@/components/graph/GraphView").then((m) => ({
    default: m.GraphView,
  })),
);
const HomeDashboard = lazy(() =>
  import("@/components/home/HomeDashboard").then((m) => ({
    default: m.HomeDashboard,
  })),
);
const PlannerView = lazy(() =>
  import("@/components/planner/PlannerView").then((m) => ({
    default: m.PlannerView,
  })),
);

/** Content router, dispatches the active pane to its surface. */
export function ContentPane() {
  const pane = useUi((s) => s.activePane);

  let body: React.ReactNode;
  switch (pane.kind) {
    case "page":
      body = <PageRouter pageId={pane.pageId} />;
      break;
    case "settings":
      body = <SettingsView />;
      break;
    case "meeting":
      body = <MeetingRecorder />;
      break;
    case "graph":
      body = <GraphView />;
      break;
    case "planner":
      body = <PlannerView />;
      break;
    default:
      body = <HomeDashboard />;
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <Suspense fallback={<div className="p-12 text-sm text-text-faint">Loading…</div>}>
        {body}
      </Suspense>
    </div>
  );
}
