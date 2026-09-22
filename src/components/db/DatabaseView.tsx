import { useCallback, useMemo } from "react";
import { Table2, Columns3, CalendarDays, GanttChartSquare } from "lucide-react";
import { useDatabase, useDbMutations, useRelationTargets } from "@/hooks/useDatabase";
import { useUi } from "@/store/ui";
import type { CellValue, DbView, Field, RowWithCells, ViewConfig } from "@/lib/api";
import { applyView, type Filter, type Sort } from "@/lib/dbFilters";
import { Skeleton } from "@/components/ui/skeleton";
import { GridView } from "./GridView";
import { BoardView } from "./BoardView";
import { CalendarView } from "./CalendarView";
import { GanttView } from "./GanttView";
import { DbToolbar } from "./DbToolbar";
import { pruneViewConfig, resolveCellValue } from "./viewFields";

const VIEW_META: Record<
  DbView["kind"],
  { label: string; icon: React.ReactNode }
> = {
  grid: { label: "Grid", icon: <Table2 className="size-3.5" /> },
  board: { label: "Board", icon: <Columns3 className="size-3.5" /> },
  calendar: { label: "Calendar", icon: <CalendarDays className="size-3.5" /> },
  gantt: { label: "Gantt", icon: <GanttChartSquare className="size-3.5" /> },
};

export function DatabaseView({ pageId }: { pageId: string }) {
  const { data: bundle, isLoading, isError } = useDatabase(pageId);
  const m = useDbMutations(pageId);
  const targetBundles = useRelationTargets(bundle);
  const active = useUi((s) =>
    bundle ? s.dbActiveView[bundle.database_id] : undefined,
  );
  const setActive = useUi((s) => s.setDbActiveView);

  const views = bundle?.views ?? [];
  const activeView = views.find((v) => v.id === active) ?? views[0] ?? null;
  const fields = bundle?.fields;
  const rows = bundle?.rows;
  const filters = activeView?.config?.filters;
  const sorts = activeView?.config?.sorts;

  /**
   * Filters and sorts run against what the user sees: computed columns are
   * evaluated here, and relations read as their row labels.
   */
  const resolve = useCallback(
    (row: RowWithCells, field: Field): CellValue =>
      fields
        ? resolveCellValue(row, field, fields, targetBundles)
        : row.cells[field.id],
    [fields, targetBundles],
  );

  // Filtering + sorting every row is the expensive part of a render, so it only
  // re-runs when the rows, the view's filters/sorts or the relation data change.
  const viewRows = useMemo(
    () =>
      rows && fields
        ? applyView(
            rows,
            fields,
            (filters ?? []) as Filter[],
            (sorts ?? []) as Sort[],
            resolve,
          )
        : [],
    [rows, fields, filters, sorts, resolve],
  );

  const viewBundle = useMemo(
    () => (bundle ? { ...bundle, rows: viewRows } : bundle),
    [bundle, viewRows],
  );

  if (isLoading) {
    return (
      <div className="space-y-3 p-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }
  if (isError || !bundle || !viewBundle) {
    return <div className="p-6 text-danger-c">Couldn't load this database.</div>;
  }

  const updateConfig = (config: ViewConfig) => {
    // Drop anything still pointing at a deleted field before it is written back.
    if (activeView)
      m.updateView.mutate({
        id: activeView.id,
        config: pruneViewConfig(config, bundle.fields),
      });
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-1 border-b border-border px-3 py-1.5">
        {views.map((v) => (
          <button
            key={v.id}
            role="tab"
            aria-selected={activeView?.id === v.id}
            onClick={() => setActive(bundle.database_id, v.id)}
            className={`flex items-center gap-1.5 rounded-md px-2.5 py-1 text-note transition-colors ${
              activeView?.id === v.id
                ? "bg-brand-soft font-semibold text-text"
                : "text-text-muted hover:bg-surface-hover hover:text-text"
            }`}
          >
            {VIEW_META[v.kind].icon}
            {VIEW_META[v.kind].label}
          </button>
        ))}
        {activeView && (
          <div className="ml-auto">
            <DbToolbar bundle={bundle} view={activeView} onUpdate={updateConfig} />
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1">
        {activeView?.kind === "grid" && (
          <GridView
            key={activeView.id}
            bundle={viewBundle}
            pageId={pageId}
            view={activeView}
            onUpdate={updateConfig}
            targetBundles={targetBundles}
          />
        )}
        {activeView?.kind === "board" && (
          <BoardView
            bundle={viewBundle}
            view={activeView}
            pageId={pageId}
            allRows={bundle.rows}
            targetBundles={targetBundles}
          />
        )}
        {activeView?.kind === "calendar" && (
          <CalendarView
            bundle={viewBundle}
            view={activeView}
            targetBundles={targetBundles}
          />
        )}
        {activeView?.kind === "gantt" && (
          <GanttView
            bundle={viewBundle}
            view={activeView}
            targetBundles={targetBundles}
          />
        )}
      </div>
    </div>
  );
}
