import { describe, it, expect } from "vitest";
import { applyView, type Filter, type Sort } from "@/lib/dbFilters";
import type { Field, RowWithCells } from "@/lib/api";

const fields: Field[] = [
  { id: "name", database_id: "d", name: "Name", type: "text", options: null, position: 1 },
  { id: "n", database_id: "d", name: "N", type: "number", options: null, position: 2 },
  { id: "done", database_id: "d", name: "Done", type: "checkbox", options: null, position: 3 },
];
const rows: RowWithCells[] = [
  { id: "1", database_id: "d", position: 1, created_at: 0, cells: { name: "Banana", n: 3, done: true } },
  { id: "2", database_id: "d", position: 2, created_at: 0, cells: { name: "Apple", n: 1, done: false } },
  { id: "3", database_id: "d", position: 3, created_at: 0, cells: { name: "Cherry", n: 2, done: true } },
];

describe("applyView", () => {
  it("filters by text contains", () => {
    const f: Filter[] = [{ fieldId: "name", op: "contains", value: "an" }];
    expect(applyView(rows, fields, f).map((r) => r.id)).toEqual(["1"]); // Banana
  });

  it("filters checkbox checked", () => {
    const f: Filter[] = [{ fieldId: "done", op: "checked", value: true }];
    expect(applyView(rows, fields, f).map((r) => r.id).sort()).toEqual(["1", "3"]);
  });

  it("filters number gt", () => {
    const f: Filter[] = [{ fieldId: "n", op: "gt", value: 1 }];
    expect(applyView(rows, fields, f).map((r) => r.id).sort()).toEqual(["1", "3"]);
  });

  it("sorts by name asc and number desc", () => {
    const sName: Sort[] = [{ fieldId: "name", dir: "asc" }];
    expect(applyView(rows, fields, [], sName).map((r) => r.cells.name)).toEqual([
      "Apple",
      "Banana",
      "Cherry",
    ]);
    const sNum: Sort[] = [{ fieldId: "n", dir: "desc" }];
    expect(applyView(rows, fields, [], sNum).map((r) => r.cells.n)).toEqual([3, 2, 1]);
  });

  it("combines filter + sort", () => {
    const out = applyView(
      rows,
      fields,
      [{ fieldId: "done", op: "checked", value: true }],
      [{ fieldId: "name", dir: "asc" }],
    );
    expect(out.map((r) => r.cells.name)).toEqual(["Banana", "Cherry"]);
  });
});
