import type { Field, RowWithCells, ViewConfig, CellValue } from "@/lib/api";
import { choiceById } from "@/components/db/cells";
import { addDays, dayNum, parseLocalDate } from "@/lib/dates";

// The date helpers live in `@/lib/dates` now (shared with the cell renderers),
// but they are re-exported here so existing gantt callers keep working.
export { addDays, dayNum, parseLocalDate };

export interface GanttTask {
  id: string;
  name: string;
  start: Date;
  end: Date;
  color: string;
  deps: string[];
}

export interface GanttModel {
  tasks: GanttTask[];
  min: Date;
  totalDays: number;
}

/** Read a cell, optionally through a resolver (so computed fields work). */
export type ValueResolver = (row: RowWithCells, field: Field) => CellValue;

const rawValue: ValueResolver = (row, field) => row.cells[field.id];

export function colorFor(color?: string): string {
  switch (color) {
    case "blue":
      return "#3b6fb0";
    case "green":
      return "var(--success)";
    case "amber":
      return "var(--warning)";
    case "red":
      return "var(--danger-c)";
    default:
      return "var(--brand)";
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function asArray(v: any): unknown[] {
  if (Array.isArray(v)) return v;
  if (typeof v === "string") {
    try {
      const p = JSON.parse(v);
      return Array.isArray(p) ? p : [];
    } catch {
      return [];
    }
  }
  return [];
}

/** Build the pure Gantt model (tasks + date window) from rows + view config. */
export function buildGanttModel(
  rows: RowWithCells[],
  fields: Field[],
  cfg: ViewConfig,
  resolve: ValueResolver = rawValue,
): GanttModel {
  const byId = (id: string | undefined) =>
    id ? fields.find((f) => f.id === id) : undefined;
  const nameField = fields.find((f) => f.type === "text") ?? fields[0];
  // A config can point at a field that has since been deleted, so every lookup
  // falls back to the first field of the right type.
  const startField =
    byId(cfg.startFieldId) ?? fields.find((f) => f.type === "date");
  const endField = byId(cfg.endFieldId);
  const statusField = fields.find((f) => f.type === "select");
  const depsField =
    byId(cfg.dependenciesFieldId) ??
    fields.find((f) => f.type === "dependencies");

  if (!startField) return { tasks: [], min: new Date(0), totalDays: 0 };

  const tasks: GanttTask[] = [];
  for (const r of rows) {
    const start = parseLocalDate(resolve(r, startField));
    if (!start) continue;
    const rawEnd = endField ? parseLocalDate(resolve(r, endField)) : null;
    const end = rawEnd && rawEnd >= start ? rawEnd : start;
    const choice = statusField
      ? choiceById(statusField, r.cells[statusField.id])
      : undefined;
    const deps = depsField ? (asArray(r.cells[depsField.id]) as string[]) : [];
    const name = nameField ? resolve(r, nameField) : null;
    tasks.push({
      id: r.id,
      name: name == null || name === "" ? "Untitled" : String(name),
      start,
      end,
      color: colorFor(choice?.color),
      deps,
    });
  }
  if (tasks.length === 0) return { tasks, min: new Date(0), totalDays: 0 };

  let lo = tasks[0].start;
  let hi = tasks[0].end;
  for (const t of tasks) {
    if (t.start < lo) lo = t.start;
    if (t.end > hi) hi = t.end;
  }
  const min = addDays(lo, -2);
  const totalDays = dayNum(hi) - dayNum(min) + 4;
  return { tasks, min, totalDays };
}
