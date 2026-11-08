import { useCallback, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { GanttChartSquare } from "lucide-react";
import type { CellValue, DatabaseBundle, DbView, Field, RowWithCells } from "@/lib/api";
import { buildGanttModel, dayNum, addDays } from "@/lib/gantt";
import { isComputed, computeCell } from "@/lib/computed";
import { DbEmptyState, MissingFieldNotice } from "./EmptyState";
import { ganttFields } from "./viewFields";

const DAY = 28; // px per day
const ROW_H = 38;
const NAME_W = 208;
const HEADER_H = 36;

export function GanttView({
  bundle,
  view,
  targetBundles = {},
}: {
  bundle: DatabaseBundle;
  view: DbView;
  targetBundles?: Record<string, DatabaseBundle>;
}) {
  const { fields, rows } = bundle;
  // `view.config` is stable between renders (it comes straight off the cached
  // bundle); a fresh `{}` fallback would re-run every memo on every render.
  const cfg = view.config;
  const scrollRef = useRef<HTMLDivElement>(null);

  const resolve = useCallback(
    (row: RowWithCells, field: Field): CellValue =>
      isComputed(field.type)
        ? computeCell(field, row, fields, targetBundles)
        : row.cells[field.id],
    [fields, targetBundles],
  );

  const { tasks, min, totalDays } = useMemo(
    () => buildGanttModel(rows, fields, cfg ?? {}, resolve),
    [rows, fields, cfg, resolve],
  );

  const rowVirtualizer = useVirtualizer({
    count: tasks.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
  });

  const width = totalDays * DAY;
  const xOf = useCallback(
    (d: Date) => (dayNum(d) - dayNum(min)) * DAY,
    [min],
  );

  // month header segments
  const months = useMemo(() => {
    const out: { label: string; left: number; w: number }[] = [];
    if (totalDays <= 0) return out;
    let cursor = new Date(min);
    while (dayNum(cursor) < dayNum(min) + totalDays) {
      const monthStart = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
      const monthEnd = new Date(cursor.getFullYear(), cursor.getMonth() + 1, 1);
      const segStart = cursor > min ? cursor : min;
      const left = xOf(segStart);
      const right = Math.min(xOf(monthEnd), width);
      out.push({
        label: monthStart.toLocaleDateString(undefined, {
          month: "short",
          year: "2-digit",
        }),
        left,
        w: right - left,
      });
      cursor = monthEnd;
    }
    return out;
  }, [min, totalDays, width, xOf]);

  const gantt = ganttFields(fields, cfg);
  const missing = gantt.start.missing || gantt.end.missing || gantt.deps.missing;

  if (!gantt.start.field) {
    return (
      <div className="p-6">
        {missing && <MissingFieldNotice className="mb-4" />}
        <DbEmptyState
          icon={<GanttChartSquare className="size-5" />}
          title="No start date field yet"
          hint="A timeline needs a Date field to start each bar. Add one, then choose it in View settings."
        />
      </div>
    );
  }
  if (tasks.length === 0) {
    return (
      <div className="p-6">
        {missing && <MissingFieldNotice className="mb-4" />}
        <DbEmptyState
          icon={<GanttChartSquare className="size-5" />}
          title="Nothing on the timeline"
          hint={`No rows have a date in "${gantt.start.field.name}" yet.`}
        />
      </div>
    );
  }

  const pos = new Map(tasks.map((t, i) => [t.id, i]));
  const todayX = xOf(new Date());
  const bodyH = rowVirtualizer.getTotalSize();
  const items = rowVirtualizer.getVirtualItems();
  const firstVisible = items.length ? items[0].index : 0;
  const lastVisible = items.length ? items[items.length - 1].index : 0;

  return (
    <div className="flex h-full flex-col">
      {missing && (
        <div className="px-4 pt-3">
          <MissingFieldNotice />
        </div>
      )}
      {/* One scroller for names + timeline, so the rows stay aligned. */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto">
        <div style={{ width: NAME_W + width, position: "relative" }}>
          {/* header */}
          <div
            className="sticky top-0 z-30 flex bg-bg"
            style={{ height: HEADER_H }}
          >
            <div
              className="sticky left-0 z-40 shrink-0 border-r border-b border-border bg-bg px-3 py-2 text-xs font-semibold uppercase tracking-wide text-text-faint"
              style={{ width: NAME_W }}
            >
              Task
            </div>
            <div
              className="relative shrink-0 border-b border-border"
              style={{ width }}
            >
              {months.map((mo, i) => (
                <div
                  key={i}
                  className="absolute top-0 h-full border-r border-border px-2 py-2 text-xs font-semibold text-text-faint"
                  style={{ left: mo.left, width: mo.w }}
                >
                  {mo.label}
                </div>
              ))}
            </div>
          </div>

          {/* body */}
          <div style={{ height: bodyH, position: "relative" }}>
            {/* today marker */}
            {todayX >= 0 && todayX <= width && (
              <div
                className="pointer-events-none absolute top-0 z-0 w-px bg-brand/50"
                style={{ left: NAME_W + todayX, height: bodyH }}
              />
            )}

            {/* dependency arrows (only around the rows on screen) */}
            <svg
              className="pointer-events-none absolute top-0 z-0"
              style={{ left: NAME_W, width, height: bodyH }}
            >
              {tasks.flatMap((t) => {
                const to = pos.get(t.id)!;
                return t.deps
                  .filter((d) => {
                    const from = pos.get(d);
                    if (from === undefined) return false;
                    const lo = Math.min(from, to);
                    const hi = Math.max(from, to);
                    return hi >= firstVisible - 10 && lo <= lastVisible + 10;
                  })
                  .map((d) => {
                    const from = tasks[pos.get(d)!];
                    const x1 = xOf(addDays(from.end, 1));
                    const y1 = pos.get(d)! * ROW_H + ROW_H / 2;
                    const x2 = xOf(t.start);
                    const y2 = to * ROW_H + ROW_H / 2;
                    return (
                      <path
                        key={`${d}-${t.id}`}
                        d={`M ${x1} ${y1} C ${x1 + 16} ${y1}, ${x2 - 16} ${y2}, ${x2} ${y2}`}
                        stroke="var(--text-faint)"
                        strokeWidth={1.5}
                        fill="none"
                      />
                    );
                  });
              })}
            </svg>

            {items.map((vi) => {
              const t = tasks[vi.index];
              const left = xOf(t.start);
              const w = Math.max(
                DAY - 6,
                (dayNum(t.end) - dayNum(t.start) + 1) * DAY - 6,
              );
              return (
                <div
                  key={t.id}
                  className="absolute left-0 flex"
                  style={{
                    width: NAME_W + width,
                    height: ROW_H,
                    transform: `translateY(${vi.start}px)`,
                  }}
                >
                  <div
                    className="sticky left-0 z-20 flex shrink-0 items-center border-r border-b border-border bg-bg px-3"
                    style={{ width: NAME_W, height: ROW_H }}
                  >
                    <span className="truncate text-sm" title={t.name}>
                      {t.name}
                    </span>
                  </div>
                  <div
                    className="relative shrink-0 border-b border-border"
                    style={{ width, height: ROW_H }}
                  >
                    <div
                      title={t.name}
                      className="absolute top-1.5 flex items-center rounded-md px-2 text-xs font-medium text-on-colour"
                      style={{
                        left: left + 3,
                        width: w,
                        height: ROW_H - 12,
                        background: t.color,
                      }}
                    >
                      <span className="truncate">{t.name}</span>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
