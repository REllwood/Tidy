import { describe, it, expect } from "vitest";
import {
  asIds,
  boardGroupField,
  calendarDateField,
  ganttFields,
  linkText,
  pruneViewConfig,
  resolveCellValue,
  resolveViewField,
  rowLabel,
  titleField,
} from "@/components/db/viewFields";
import type { DatabaseBundle, Field, RowWithCells, ViewConfig } from "@/lib/api";

const f = (id: string, type: Field["type"]): Field => ({
  id,
  database_id: "d",
  name: id,
  type,
  options: null,
  position: 1,
});

const fields = [
  f("name", "text"),
  f("status", "select"),
  f("stage", "select"),
  f("start", "date"),
  f("due", "date"),
  f("deps", "dependencies"),
];

describe("resolveViewField", () => {
  it("uses the configured field when it is still there", () => {
    const r = resolveViewField(fields, "stage", (x) => x.type === "select");
    expect(r.field?.id).toBe("stage");
    expect(r.missing).toBe(false);
  });
  it("falls back to the first field of the type and flags the deletion", () => {
    const r = resolveViewField(fields, "gone", (x) => x.type === "select");
    expect(r.field?.id).toBe("status");
    expect(r.missing).toBe(true);
  });
  it("falls back without flagging when nothing was configured", () => {
    const r = resolveViewField(fields, undefined, (x) => x.type === "date");
    expect(r.field?.id).toBe("start");
    expect(r.missing).toBe(false);
  });
  it("has no field at all when none of the right type exists", () => {
    const r = resolveViewField([f("only", "text")], "gone", (x) => x.type === "select");
    expect(r.field).toBeUndefined();
    expect(r.missing).toBe(true);
  });
});

describe("per-view resolution", () => {
  it("board groups by a select field, calendar maps onto a date field", () => {
    expect(boardGroupField(fields, null).field?.id).toBe("status");
    expect(boardGroupField(fields, { groupByFieldId: "stage" }).field?.id).toBe("stage");
    expect(calendarDateField(fields, null).field?.id).toBe("start");
    const deleted = calendarDateField(fields, { dateFieldId: "gone" });
    expect(deleted.field?.id).toBe("start");
    expect(deleted.missing).toBe(true);
  });

  it("gantt falls back for start and dependencies but not for end", () => {
    const g = ganttFields(fields, { startFieldId: "gone", endFieldId: "gone" });
    expect(g.start.field?.id).toBe("start");
    expect(g.start.missing).toBe(true);
    expect(g.end.field).toBeUndefined();
    expect(g.end.missing).toBe(true);
    expect(g.deps.field?.id).toBe("deps");
  });

  it("titles rows by the first text field, else the first field", () => {
    expect(titleField(fields)?.id).toBe("name");
    expect(titleField([f("n", "number"), f("t", "text")])?.id).toBe("t");
    expect(titleField([f("n", "number")])?.id).toBe("n");
    expect(titleField([])).toBeUndefined();
  });
});

describe("pruneViewConfig", () => {
  it("drops every reference to fields that no longer exist", () => {
    const cfg: ViewConfig = {
      groupByFieldId: "gone",
      dateFieldId: "start",
      startFieldId: "gone",
      filters: [
        { fieldId: "gone", op: "contains", value: "x" },
        { fieldId: "name", op: "contains", value: "y" },
      ],
      sorts: [{ fieldId: "gone", dir: "asc" }, { fieldId: "name", dir: "desc" }],
      columnWidths: { gone: 120, name: 240 },
    };
    const next = pruneViewConfig(cfg, fields);
    expect(next.groupByFieldId).toBeUndefined();
    expect(next.startFieldId).toBeUndefined();
    expect(next.dateFieldId).toBe("start");
    expect(next.filters?.map((x) => x.fieldId)).toEqual(["name"]);
    expect(next.sorts?.map((x) => x.fieldId)).toEqual(["name"]);
    expect(next.columnWidths).toEqual({ name: 240 });
  });
  it("copes with a null config", () => {
    expect(pruneViewConfig(null, fields)).toEqual({});
  });
});

// --- link + computed value resolution --------------------------------------
const row = (id: string, cells: Record<string, unknown>): RowWithCells => ({
  id,
  database_id: "d",
  position: 1,
  created_at: 0,
  cells,
});

const clients: DatabaseBundle = {
  database_id: "clients",
  fields: [
    { id: "c_name", database_id: "clients", name: "Name", type: "text", options: null, position: 1 },
  ],
  rows: [row("acme", { c_name: "Acme Corp" }), row("blank", {})],
  views: [],
};
const targets = { clients };
const relation: Field = {
  id: "client",
  database_id: "d",
  name: "Client",
  type: "relation",
  options: { targetDatabaseId: "clients" },
  position: 1,
};

describe("asIds / rowLabel", () => {
  it("reads an array, a JSON string, or nothing", () => {
    expect(asIds(["a", "b"])).toEqual(["a", "b"]);
    expect(asIds('["a"]')).toEqual(["a"]);
    expect(asIds("not json")).toEqual([]);
    expect(asIds(null)).toEqual([]);
  });
  it("labels a row by its primary field, falling back to Untitled", () => {
    expect(rowLabel(clients, clients.rows[0])).toBe("Acme Corp");
    expect(rowLabel(clients, clients.rows[1])).toBe("Untitled");
    expect(rowLabel(undefined, clients.rows[0])).toBe("Untitled");
  });
});

describe("resolveCellValue / linkText", () => {
  it("resolves a relation to the linked rows' names", () => {
    expect(resolveCellValue(row("r", { client: ["acme"] }), relation, [relation], targets)).toEqual([
      "Acme Corp",
    ]);
    expect(linkText(row("r", { client: ["acme"] }), relation, targets)).toBe("Acme Corp");
  });
  it("never shows raw ids while the related database is loading", () => {
    expect(linkText(row("r", { client: ["acme", "globex"] }), relation, {})).toBe("2 linked");
    expect(linkText(row("r", { client: [] }), relation, {})).toBe("");
  });
  it("counts dependencies, which have no target database", () => {
    const deps: Field = { ...relation, id: "deps", type: "dependencies", options: null };
    expect(linkText(row("r", { deps: ["a", "b"] }), deps, targets)).toBe("2 linked");
  });
  it("evaluates computed fields and passes stored cells through", () => {
    const budget: Field = { id: "b", database_id: "d", name: "Budget", type: "number", options: null, position: 1 };
    const vat: Field = {
      id: "vat",
      database_id: "d",
      name: "VAT",
      type: "formula",
      options: { expr: "{Budget} * 2" },
      position: 2,
    };
    const r = row("r", { b: 50 });
    expect(resolveCellValue(r, vat, [budget, vat], {})).toBe(100);
    expect(resolveCellValue(r, budget, [budget, vat], {})).toBe(50);
  });
});
