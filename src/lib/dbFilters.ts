import type { Field, RowWithCells, CellValue } from "@/lib/api";
import { dayKey } from "@/lib/dates";

export type FilterOp =
  | "contains"
  | "equals"
  | "is"
  | "is_not"
  | "is_empty"
  | "is_not_empty"
  | "checked"
  | "unchecked"
  | "gt"
  | "lt"
  | "before"
  | "after"
  | "on";

export interface Filter {
  fieldId: string;
  op: FilterOp;
  value: CellValue;
}
export interface Sort {
  fieldId: string;
  dir: "asc" | "desc";
}

/**
 * Resolve the value a filter or sort should see for a cell. Stored fields read
 * straight from the row; computed fields (lookup/rollup/formula) and relations
 * are handed in by the view so they filter and sort on what the user sees.
 */
export type ValueResolver = (row: RowWithCells, field: Field) => CellValue;

const rawValue: ValueResolver = (row, field) => row.cells[field.id];

/** Human labels for the operator picker. */
export const OP_LABELS: Record<FilterOp, string> = {
  contains: "Contains",
  equals: "Equals",
  is: "Is",
  is_not: "Is not",
  is_empty: "Is empty",
  is_not_empty: "Is not empty",
  checked: "Checked",
  unchecked: "Unchecked",
  gt: "Greater than",
  lt: "Less than",
  before: "Before",
  after: "After",
  on: "On",
};

const TEXT_OPS: FilterOp[] = ["contains", "equals", "is_empty", "is_not_empty"];
const NUMBER_OPS: FilterOp[] = ["equals", "gt", "lt", "is_empty"];
const DATE_OPS: FilterOp[] = ["after", "before", "on", "is_empty"];
const SELECT_OPS: FilterOp[] = ["is", "is_not", "is_empty"];
const CHECKBOX_OPS: FilterOp[] = ["checked", "unchecked"];
const LINK_OPS: FilterOp[] = ["contains", "is_empty"];
const COMPUTED_OPS: FilterOp[] = [
  "contains",
  "equals",
  "gt",
  "lt",
  "is_empty",
  "is_not_empty",
];

/** The operators offered for a field type, best first. */
export function opsForType(type: Field["type"]): FilterOp[] {
  switch (type) {
    case "number":
      return NUMBER_OPS;
    case "date":
      return DATE_OPS;
    case "select":
      return SELECT_OPS;
    case "checkbox":
      return CHECKBOX_OPS;
    case "relation":
    case "dependencies":
      return LINK_OPS;
    case "lookup":
    case "rollup":
    case "formula":
      return COMPUTED_OPS;
    default:
      return TEXT_OPS;
  }
}

/** Default filter op for a field type. */
export function defaultOp(type: Field["type"]): FilterOp {
  return opsForType(type)[0];
}

/** Ops that make no use of a typed value (so they run without one). */
export function isValueless(op: FilterOp): boolean {
  return (
    op === "checked" ||
    op === "unchecked" ||
    op === "is_empty" ||
    op === "is_not_empty"
  );
}

function isEmpty(value: CellValue): boolean {
  if (value == null || value === "") return true;
  if (Array.isArray(value)) return value.length === 0;
  return false;
}

/** Flatten a cell into text; arrays (relations, multi-lookups) read as a list. */
function asText(value: CellValue): string {
  if (value == null) return "";
  if (Array.isArray(value)) return value.map((v) => String(v ?? "")).join(", ");
  return String(value);
}

const numeric = (v: CellValue): number | null => {
  if (v == null || v === "" || typeof v === "boolean" || Array.isArray(v))
    return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
};

function matches(value: CellValue, f: Filter, type: Field["type"]): boolean {
  // Value-based ops are a no-op until the user supplies a value (so adding a
  // filter doesn't instantly hide every row).
  if (!isValueless(f.op) && (f.value == null || f.value === "")) return true;

  switch (f.op) {
    case "contains":
      return asText(value)
        .toLowerCase()
        .includes(String(f.value ?? "").toLowerCase());
    case "equals": {
      const a = numeric(value);
      const b = numeric(f.value);
      if (type === "number" || (a !== null && b !== null)) return a === b;
      return asText(value) === asText(f.value);
    }
    case "is":
      return value === f.value;
    case "is_not":
      return value !== f.value;
    case "is_empty":
      return isEmpty(value);
    case "is_not_empty":
      return !isEmpty(value);
    case "checked":
      return !!value;
    case "unchecked":
      return !value;
    case "gt": {
      const a = numeric(value);
      const b = numeric(f.value);
      return a !== null && b !== null && a > b;
    }
    case "lt": {
      const a = numeric(value);
      const b = numeric(f.value);
      return a !== null && b !== null && a < b;
    }
    case "before": {
      const a = dayKey(value);
      const b = dayKey(f.value);
      return a !== null && b !== null && a < b;
    }
    case "after": {
      const a = dayKey(value);
      const b = dayKey(f.value);
      return a !== null && b !== null && a > b;
    }
    case "on": {
      const a = dayKey(value);
      const b = dayKey(f.value);
      return a !== null && b !== null && a === b;
    }
    default:
      return true;
  }
}

function compare(a: CellValue, b: CellValue, type: Field["type"]): number {
  if (isEmpty(a) && isEmpty(b)) return 0;
  if (isEmpty(a)) return -1;
  if (isEmpty(b)) return 1;
  if (type === "checkbox") return (a ? 1 : 0) - (b ? 1 : 0);
  if (type === "date") {
    const da = dayKey(a);
    const db = dayKey(b);
    if (da !== null && db !== null) return da - db;
  }
  const na = numeric(a);
  const nb = numeric(b);
  if (type === "number" || (na !== null && nb !== null))
    return (na ?? 0) - (nb ?? 0);
  return asText(a).localeCompare(asText(b));
}

/**
 * Apply a view's filters (AND) then sorts to a row set. `resolve` lets the
 * caller supply computed values so lookup/rollup/formula columns filter and
 * sort on what is shown, not on the (absent) stored cell.
 */
export function applyView(
  rows: RowWithCells[],
  fields: Field[],
  filters: Filter[] = [],
  sorts: Sort[] = [],
  resolve: ValueResolver = rawValue,
): RowWithCells[] {
  const byId = new Map(fields.map((f) => [f.id, f]));
  let out = rows.filter((r) =>
    filters.every((f) => {
      const field = byId.get(f.fieldId);
      // A filter can outlive the field it referenced; ignore it rather than
      // hiding every row.
      if (!field) return true;
      return matches(resolve(r, field), f, field.type);
    }),
  );
  for (const s of [...sorts].reverse()) {
    const field = byId.get(s.fieldId);
    if (!field) continue;
    out = [...out].sort((ra, rb) => {
      const c = compare(resolve(ra, field), resolve(rb, field), field.type);
      return s.dir === "asc" ? c : -c;
    });
  }
  return out;
}
