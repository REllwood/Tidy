import { describe, it, expect } from "vitest";
import {
  applyView,
  defaultOp,
  isValueless,
  opsForType,
  OP_LABELS,
  type Filter,
  type FilterOp,
  type Sort,
} from "@/lib/dbFilters";
import type { Field, RowWithCells } from "@/lib/api";

const fields: Field[] = [
  { id: "name", database_id: "d", name: "Name", type: "text", options: null, position: 1 },
  { id: "n", database_id: "d", name: "N", type: "number", options: null, position: 2 },
  { id: "done", database_id: "d", name: "Done", type: "checkbox", options: null, position: 3 },
  { id: "due", database_id: "d", name: "Due", type: "date", options: null, position: 4 },
  {
    id: "status",
    database_id: "d",
    name: "Status",
    type: "select",
    options: { choices: [{ id: "todo", name: "To do", color: "grey" }] },
    position: 5,
  },
  { id: "client", database_id: "d", name: "Client", type: "relation", options: null, position: 6 },
  {
    id: "total",
    database_id: "d",
    name: "Total",
    type: "rollup",
    options: { relationFieldId: "client", targetFieldId: "x", fn: "sum" },
    position: 7,
  },
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

  it("ignores a filter or sort whose field has been deleted", () => {
    const f: Filter[] = [{ fieldId: "gone", op: "contains", value: "zzz" }];
    expect(applyView(rows, fields, f).map((r) => r.id)).toEqual(["1", "2", "3"]);
    expect(
      applyView(rows, fields, [], [{ fieldId: "gone", dir: "asc" }]).map((r) => r.id),
    ).toEqual(["1", "2", "3"]);
  });
});

describe("operators", () => {
  const ids = (f: Filter[], rs = rows) => applyView(rs, fields, f).map((r) => r.id);

  it("offers the right operators per field type, best first", () => {
    expect(opsForType("text")).toEqual([
      "contains",
      "equals",
      "is_empty",
      "is_not_empty",
    ]);
    expect(opsForType("number")).toEqual(["equals", "gt", "lt", "is_empty"]);
    expect(opsForType("date")).toEqual(["after", "before", "on", "is_empty"]);
    expect(opsForType("select")).toEqual(["is", "is_not", "is_empty"]);
    expect(opsForType("checkbox")).toEqual(["checked", "unchecked"]);
    expect(opsForType("relation")).toEqual(["contains", "is_empty"]);
    for (const t of ["text", "number", "date", "select", "checkbox"] as const)
      expect(defaultOp(t)).toBe(opsForType(t)[0]);
  });

  it("labels every operator it offers", () => {
    const all = new Set<FilterOp>();
    (
      ["text", "number", "date", "select", "checkbox", "relation", "rollup"] as const
    ).forEach((t) => opsForType(t).forEach((o) => all.add(o)));
    for (const op of all) expect(OP_LABELS[op]).toBeTruthy();
  });

  it("treats only the valueless ops as needing no value", () => {
    expect(isValueless("is_empty")).toBe(true);
    expect(isValueless("is_not_empty")).toBe(true);
    expect(isValueless("checked")).toBe(true);
    expect(isValueless("unchecked")).toBe(true);
    expect(isValueless("contains")).toBe(false);
  });

  it("is empty / is not empty cover null, empty string and empty arrays", () => {
    const rs: RowWithCells[] = [
      { id: "a", database_id: "d", position: 1, created_at: 0, cells: { name: "set" } },
      { id: "b", database_id: "d", position: 2, created_at: 0, cells: { name: "" } },
      { id: "c", database_id: "d", position: 3, created_at: 0, cells: {} },
      { id: "e", database_id: "d", position: 4, created_at: 0, cells: { name: [] } },
    ];
    expect(ids([{ fieldId: "name", op: "is_empty", value: "" }], rs)).toEqual(["b", "c", "e"]);
    expect(ids([{ fieldId: "name", op: "is_not_empty", value: "" }], rs)).toEqual(["a"]);
  });

  it("select is / is not", () => {
    const rs: RowWithCells[] = [
      { id: "a", database_id: "d", position: 1, created_at: 0, cells: { status: "todo" } },
      { id: "b", database_id: "d", position: 2, created_at: 0, cells: { status: "done" } },
    ];
    expect(ids([{ fieldId: "status", op: "is", value: "todo" }], rs)).toEqual(["a"]);
    expect(ids([{ fieldId: "status", op: "is_not", value: "todo" }], rs)).toEqual(["b"]);
  });

  it("date before / after / on compare local calendar days", () => {
    const rs: RowWithCells[] = [
      { id: "a", database_id: "d", position: 1, created_at: 0, cells: { due: "2026-06-10" } },
      { id: "b", database_id: "d", position: 2, created_at: 0, cells: { due: "2026-06-20" } },
      { id: "c", database_id: "d", position: 3, created_at: 0, cells: { due: "2026-06-30" } },
    ];
    expect(ids([{ fieldId: "due", op: "before", value: "2026-06-20" }], rs)).toEqual(["a"]);
    expect(ids([{ fieldId: "due", op: "after", value: "2026-06-20" }], rs)).toEqual(["c"]);
    expect(ids([{ fieldId: "due", op: "on", value: "2026-06-20" }], rs)).toEqual(["b"]);
    // a full timestamp on the same local day still counts as "on"
    expect(ids([{ fieldId: "due", op: "on", value: "2026-06-20T18:00:00" }], rs)).toEqual(["b"]);
  });

  it("number lt, and equals matching across string/number", () => {
    expect(ids([{ fieldId: "n", op: "lt", value: 3 }]).sort()).toEqual(["2", "3"]);
    expect(ids([{ fieldId: "n", op: "equals", value: "2" }])).toEqual(["3"]);
  });

  it("relation contains reads the linked list", () => {
    const rs: RowWithCells[] = [
      { id: "a", database_id: "d", position: 1, created_at: 0, cells: { client: ["Acme", "Globex"] } },
      { id: "b", database_id: "d", position: 2, created_at: 0, cells: { client: [] } },
    ];
    expect(ids([{ fieldId: "client", op: "contains", value: "globe" }], rs)).toEqual(["a"]);
    expect(ids([{ fieldId: "client", op: "is_empty", value: "" }], rs)).toEqual(["b"]);
  });

  it("a value-based op with no value yet leaves every row visible", () => {
    expect(ids([{ fieldId: "name", op: "contains", value: "" }])).toEqual(["1", "2", "3"]);
  });
});

describe("computed columns", () => {
  // Stand-in for the resolver DatabaseView passes: rollups read a derived map.
  const totals: Record<string, number> = { "1": 300, "2": 100, "3": 200 };
  const resolve = (row: RowWithCells, field: Field) =>
    field.type === "rollup" ? totals[row.id] : row.cells[field.id];

  it("filters on the computed value, not the (absent) stored cell", () => {
    const f: Filter[] = [{ fieldId: "total", op: "gt", value: 150 }];
    expect(applyView(rows, fields, f, [], resolve).map((r) => r.id).sort()).toEqual([
      "1",
      "3",
    ]);
    // without the resolver there is nothing stored to compare, so nothing matches
    expect(applyView(rows, fields, f).map((r) => r.id)).toEqual([]);
  });

  it("sorts on the computed value", () => {
    const s: Sort[] = [{ fieldId: "total", dir: "asc" }];
    expect(applyView(rows, fields, [], s, resolve).map((r) => r.id)).toEqual([
      "2",
      "3",
      "1",
    ]);
  });
});
