import type { Field, RowWithCells, ViewConfig } from "@/lib/api";
import { choiceById } from "@/components/db/cells";

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

/** Parse "YYYY-MM-DD" (or any date string) in LOCAL time. */
export function parseLocalDate(v: unknown): Date | null {
  if (v == null || v === "") return null;
  const s = String(v);
  const m = s.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (m) return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  const d = new Date(s);
  return isNaN(d.getTime()) ? null : d;
}

export const dayNum = (d: Date) => Math.floor(d.getTime() / 86_400_000);

export function addDays(d: Date, n: number): Date {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
}

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
): GanttModel {
  const nameField = fields.find((f) => f.type === "text") ?? fields[0];
  const startField =
    fields.find((f) => f.id === cfg.startFieldId) ??
    fields.find((f) => f.type === "date");
  const endField = fields.find((f) => f.id === cfg.endFieldId);
  const statusField = fields.find((f) => f.type === "select");
  const depsField =
    fields.find((f) => f.id === cfg.dependenciesFieldId) ??
    fields.find((f) => f.type === "dependencies");

  if (!startField) return { tasks: [], min: new Date(0), totalDays: 0 };

  const tasks: GanttTask[] = [];
  for (const r of rows) {
    const start = parseLocalDate(r.cells[startField.id]);
    if (!start) continue;
    const rawEnd = endField ? parseLocalDate(r.cells[endField.id]) : null;
    const end = rawEnd && rawEnd >= start ? rawEnd : start;
    const choice = statusField
      ? choiceById(statusField, r.cells[statusField.id])
      : undefined;
    const deps = depsField ? (asArray(r.cells[depsField.id]) as string[]) : [];
    tasks.push({
      id: r.id,
      name: String(r.cells[nameField.id] ?? "Untitled"),
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
