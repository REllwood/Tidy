import { useMemo } from "react";
import type { DatabaseBundle, DbView } from "@/lib/api";
import { buildGanttModel, dayNum, addDays } from "@/lib/gantt";

const DAY = 28; // px per day
const ROW_H = 38;

export function GanttView({
  bundle,
  view,
}: {
  bundle: DatabaseBundle;
  view: DbView;
}) {
  const { fields, rows } = bundle;
  const cfg = view.config ?? {};
  const startField =
    fields.find((f) => f.id === cfg.startFieldId) ??
    fields.find((f) => f.type === "date");

  const { tasks, min, totalDays } = useMemo(
    () => buildGanttModel(rows, fields, cfg),
    [rows, fields, cfg],
  );

  if (!startField) {
    return (
      <div className="p-8 text-sm text-text-faint">
        Pick a <b>Start date</b> field to see a timeline.
      </div>
    );
  }
  if (tasks.length === 0) {
    return (
      <div className="p-8 text-sm text-text-faint">
        No rows with a start date yet.
      </div>
    );
  }

  const width = totalDays * DAY;
  const pos = new Map(tasks.map((t, i) => [t.id, i]));
  const xOf = (d: Date) => (dayNum(d) - dayNum(min)) * DAY;
  const todayX = xOf(new Date());

  // month header segments
  const months: { label: string; left: number; w: number }[] = [];
  let cursor = new Date(min);
  while (dayNum(cursor) < dayNum(min) + totalDays) {
    const monthStart = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
    const monthEnd = new Date(cursor.getFullYear(), cursor.getMonth() + 1, 1);
    const segStart = cursor > min ? cursor : min;
    const left = xOf(segStart);
    const right = Math.min(xOf(monthEnd), width);
    months.push({
      label: monthStart.toLocaleDateString(undefined, {
        month: "short",
        year: "2-digit",
      }),
      left,
      w: right - left,
    });
    cursor = monthEnd;
  }

  return (
    <div className="flex h-full">
      {/* task name column */}
      <div className="w-52 shrink-0 border-r border-border">
        <div className="h-9 border-b border-border" />
        {tasks.map((t) => (
          <div
            key={t.id}
            className="flex items-center truncate border-b border-border px-3 text-sm"
            style={{ height: ROW_H }}
          >
            {t.name}
          </div>
        ))}
      </div>

      {/* timeline */}
      <div className="min-w-0 flex-1 overflow-auto">
        <div style={{ width, position: "relative" }}>
          {/* month header */}
          <div className="sticky top-0 z-10 flex h-9 border-b border-border bg-bg">
            {months.map((m, i) => (
              <div
                key={i}
                className="border-r border-border px-2 py-1 text-xs font-semibold text-text-faint"
                style={{ position: "absolute", left: m.left, width: m.w }}
              >
                {m.label}
              </div>
            ))}
          </div>

          {/* today marker */}
          {todayX >= 0 && todayX <= width && (
            <div
              className="pointer-events-none absolute top-9 z-0 w-px bg-brand/50"
              style={{ left: todayX, height: tasks.length * ROW_H }}
            />
          )}

          {/* dependency arrows */}
          <svg
            className="pointer-events-none absolute left-0"
            style={{ top: 36, width, height: tasks.length * ROW_H }}
          >
            {tasks.flatMap((t) =>
              t.deps
                .filter((d) => pos.has(d))
                .map((d) => {
                  const from = tasks[pos.get(d)!];
                  const x1 = xOf(addDays(from.end, 1));
                  const y1 = pos.get(d)! * ROW_H + ROW_H / 2;
                  const x2 = xOf(t.start);
                  const y2 = pos.get(t.id)! * ROW_H + ROW_H / 2;
                  return (
                    <path
                      key={`${d}-${t.id}`}
                      d={`M ${x1} ${y1} C ${x1 + 16} ${y1}, ${x2 - 16} ${y2}, ${x2} ${y2}`}
                      stroke="var(--text-faint)"
                      strokeWidth={1.5}
                      fill="none"
                    />
                  );
                }),
            )}
          </svg>

          {/* bars */}
          <div className="relative">
            {tasks.map((t) => {
              const left = xOf(t.start);
              const w = Math.max(
                DAY - 6,
                (dayNum(t.end) - dayNum(t.start) + 1) * DAY - 6,
              );
              return (
                <div
                  key={t.id}
                  className="relative border-b border-border"
                  style={{ height: ROW_H }}
                >
                  <div
                    title={t.name}
                    className="absolute top-1.5 flex items-center rounded-md px-2 text-xs font-medium text-white"
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
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
