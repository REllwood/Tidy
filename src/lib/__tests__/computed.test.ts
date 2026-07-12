import { describe, it, expect } from "vitest";
import {
  isComputed,
  evalFormula,
  computeCell,
  relationTargetIds,
} from "@/lib/computed";
import type { Field, RowWithCells, DatabaseBundle } from "@/lib/api";

const f = (
  id: string,
  type: Field["type"],
  name: string,
  extra: Partial<Field> = {},
): Field => ({
  id,
  database_id: "roadmap",
  name,
  type,
  options: null,
  position: 1,
  ...extra,
});

const row = (id: string, cells: Record<string, unknown>): RowWithCells => ({
  id,
  database_id: "roadmap",
  position: 1,
  created_at: 0,
  cells,
});

// --- A Clients target database (Name / Industry / Retainer) ------------------
const cName = f("c_name", "text", "Name");
const cIndustry = f("c_industry", "text", "Industry");
const cRetainer = f("c_retainer", "number", "Retainer");
const clients: DatabaseBundle = {
  database_id: "clients",
  fields: [cName, cIndustry, cRetainer],
  rows: [
    row("acme", { c_name: "Acme Corp", c_industry: "Manufacturing", c_retainer: 2000 }),
    row("globex", { c_name: "Globex", c_industry: "Finance", c_retainer: 3500 }),
  ],
  views: [],
};

// --- Roadmap fields referencing Clients --------------------------------------
const clientRel = f("client", "relation", "Client", {
  options: { targetDatabaseId: "clients", multi: true },
});
const industryLookup = f("industry", "lookup", "Client industry", {
  options: { relationFieldId: "client", targetFieldId: "c_industry" },
});
const budget = f("budget", "number", "Budget");
const vatFormula = f("vat", "formula", "Budget +VAT", {
  options: { expr: "{Budget} * 1.2" },
});
const retainerRollup = f("rollup", "rollup", "Client retainer", {
  options: { relationFieldId: "client", targetFieldId: "c_retainer", fn: "sum" },
});
const roadmapFields = [
  clientRel,
  industryLookup,
  budget,
  vatFormula,
  retainerRollup,
];
const targets = { clients };

describe("isComputed", () => {
  it("flags lookup/rollup/formula, not stored types", () => {
    expect(isComputed("lookup")).toBe(true);
    expect(isComputed("rollup")).toBe(true);
    expect(isComputed("formula")).toBe(true);
    expect(isComputed("relation")).toBe(false);
    expect(isComputed("text")).toBe(false);
    expect(isComputed("number")).toBe(false);
  });
});

describe("evalFormula", () => {
  it("multiplies a field by a constant", () => {
    expect(evalFormula("{Budget} * 1.2", roadmapFields, { budget: 1000 })).toBe(1200);
  });
  it("treats a missing/empty field as 0", () => {
    expect(evalFormula("{Budget} * 1.2", roadmapFields, {})).toBe(0);
  });
  it("supports functions and conditionals", () => {
    expect(evalFormula("round({Budget} / 3, 2)", roadmapFields, { budget: 10 })).toBe(3.33);
    expect(evalFormula("{Budget} > 100 ? 1 : 0", roadmapFields, { budget: 200 })).toBe(1);
  });
  it("supports logical && and || (returns 1/0, not 0)", () => {
    expect(evalFormula("{Budget} > 100 && {Budget} < 5000", roadmapFields, { budget: 1000 })).toBe(1);
    expect(evalFormula("{Budget} > 100 && {Budget} < 500", roadmapFields, { budget: 1000 })).toBe(0);
    expect(evalFormula("{Budget} > 5000 || {Budget} < 2000", roadmapFields, { budget: 1000 })).toBe(1);
  });
  it("concatenates when a string operand is present", () => {
    const nameF = f("nm", "text", "Name");
    expect(evalFormula("{Name} + '!'", [nameF], { nm: "Hi" })).toBe("Hi!");
  });
  it("returns null on a malformed expression", () => {
    expect(evalFormula("{Budget} * ", roadmapFields, { budget: 1 })).toBeNull();
  });
});

describe("computeCell", () => {
  const linked = row("r1", { client: ["acme"], budget: 1000 });
  const twoLinked = row("r2", { client: ["acme", "globex"], budget: 3000 });
  const unlinked = row("r3", { budget: 500 });

  it("lookup pulls the related row's field", () => {
    expect(computeCell(industryLookup, linked, roadmapFields, targets)).toBe("Manufacturing");
  });
  it("lookup joins multiple related values", () => {
    expect(computeCell(industryLookup, twoLinked, roadmapFields, targets)).toBe(
      "Manufacturing, Finance",
    );
  });
  it("rollup sums the related numeric field", () => {
    expect(computeCell(retainerRollup, linked, roadmapFields, targets)).toBe(2000);
    expect(computeCell(retainerRollup, twoLinked, roadmapFields, targets)).toBe(5500);
  });
  it("rollup count counts linked rows regardless of value type", () => {
    // count over a TEXT target field (Industry) must still count linked rows
    const countByText: Field = f("cnt", "rollup", "Client count", {
      options: { relationFieldId: "client", targetFieldId: "c_industry", fn: "count" },
    });
    expect(computeCell(countByText, linked, roadmapFields, targets)).toBe(1);
    expect(computeCell(countByText, twoLinked, roadmapFields, targets)).toBe(2);
    expect(computeCell(countByText, unlinked, roadmapFields, targets)).toBe(0);
  });
  it("formula evaluates against the row's own cells", () => {
    expect(computeCell(vatFormula, linked, roadmapFields, targets)).toBe(1200);
  });
  it("formula can reference another computed field (rollup)", () => {
    // "Budget minus retainer" = {Budget} - {Client retainer} (a rollup, sum=2000)
    const combo: Field = f("combo", "formula", "Net", {
      options: { expr: "{Budget} - {Client retainer}" },
    });
    const fieldsWithCombo = [...roadmapFields, combo];
    // Budget 1000, rollup retainer 2000 -> 1000 - 2000 = -1000
    expect(computeCell(combo, linked, fieldsWithCombo, targets)).toBe(-1000);
  });
  it("does not infinite-loop on a self-referential formula", () => {
    const cyclic: Field = f("cyc", "formula", "Cyclic", {
      options: { expr: "{Cyclic} + 1" },
    });
    expect(() =>
      computeCell(cyclic, linked, [...roadmapFields, cyclic], targets),
    ).not.toThrow();
  });
  it("yields empty/zero when nothing is linked", () => {
    expect(computeCell(industryLookup, unlinked, roadmapFields, targets)).toBeNull();
    expect(computeCell(retainerRollup, unlinked, roadmapFields, targets)).toBe(0);
  });
  it("degrades gracefully when the target bundle is absent", () => {
    expect(computeCell(retainerRollup, linked, roadmapFields, {})).toBe(0);
    expect(computeCell(industryLookup, linked, roadmapFields, {})).toBeNull();
  });
});

describe("relationTargetIds", () => {
  it("collects distinct target database ids from relation fields", () => {
    const bundle: DatabaseBundle = {
      database_id: "roadmap",
      fields: roadmapFields,
      rows: [],
      views: [],
    };
    expect(relationTargetIds(bundle)).toEqual(["clients"]);
  });
  it("is empty when there are no relation fields", () => {
    expect(relationTargetIds(clients)).toEqual([]);
  });
});
