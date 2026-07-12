import { describe, it, expect } from "vitest";
import { buildGanttModel, parseLocalDate, dayNum } from "@/lib/gantt";
import type { Field, RowWithCells } from "@/lib/api";

const f = (id: string, type: Field["type"], extra: Partial<Field> = {}): Field => ({
  id,
  database_id: "d",
  name: id,
  type,
  options: null,
  position: 1,
  ...extra,
});

const fields: Field[] = [
  f("name", "text"),
  f("status", "select", {
    options: { choices: [{ id: "done", name: "Done", color: "green" }] },
  }),
  f("start", "date"),
  f("due", "date"),
  f("deps", "dependencies"),
];

const row = (
  id: string,
  cells: Record<string, unknown>,
): RowWithCells => ({ id, database_id: "d", position: 1, created_at: 0, cells });

const cfg = { startFieldId: "start", endFieldId: "due", dependenciesFieldId: "deps" };

describe("parseLocalDate", () => {
  it("parses YYYY-MM-DD in local time (no UTC off-by-one)", () => {
    const d = parseLocalDate("2026-06-20")!;
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(5); // June
    expect(d.getDate()).toBe(20);
  });
  it("returns null for empty/invalid", () => {
    expect(parseLocalDate("")).toBeNull();
    expect(parseLocalDate(null)).toBeNull();
  });
});

describe("buildGanttModel", () => {
  it("builds tasks with start/end, color, deps and skips rows without a start", () => {
    const rows = [
      row("a", { name: "A", status: "done", start: "2026-06-10", due: "2026-06-14", deps: [] }),
      row("b", { name: "B", start: "2026-06-16", due: "2026-06-24", deps: ["a"] }),
      row("c", { name: "C" }), // no start → skipped
    ];
    const m = buildGanttModel(rows, fields, cfg);
    expect(m.tasks.map((t) => t.id)).toEqual(["a", "b"]);
    expect(m.tasks[1].deps).toEqual(["a"]);
    // A is green (Done), B falls back to brand
    expect(m.tasks[0].color).toContain("success");
    // window spans min(start)-2 .. max(end)+2 (+ padding)
    expect(dayNum(m.tasks[0].start)).toBeGreaterThan(dayNum(m.min));
    expect(m.totalDays).toBeGreaterThan(0);
  });

  it("falls back end=start when there's no due (single-day bar)", () => {
    const m = buildGanttModel(
      [row("x", { name: "X", start: "2026-07-01" })],
      fields,
      { startFieldId: "start" },
    );
    expect(dayNum(m.tasks[0].end)).toBe(dayNum(m.tasks[0].start));
  });

  it("parses a JSON-string dependencies cell", () => {
    const m = buildGanttModel(
      [
        row("a", { name: "A", start: "2026-06-10" }),
        row("b", { name: "B", start: "2026-06-12", deps: '["a"]' }),
      ],
      fields,
      cfg,
    );
    expect(m.tasks[1].deps).toEqual(["a"]);
  });

  it("returns an empty model when no rows have a start", () => {
    const m = buildGanttModel([row("a", { name: "A" })], fields, cfg);
    expect(m.tasks).toEqual([]);
    expect(m.totalDays).toBe(0);
  });
});
