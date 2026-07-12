import type { Field, RowWithCells, CellValue } from "@/lib/api";

export type FilterOp =
  | "contains"
  | "equals"
  | "is"
  | "checked"
  | "unchecked"
  | "gt"
  | "lt"
  | "before"
  | "after";

export interface Filter {
  fieldId: string;
  op: FilterOp;
  value: CellValue;
}
export interface Sort {
  fieldId: string;
  dir: "asc" | "desc";
}

/** Default filter op for a field type. */
export function defaultOp(type: Field["type"]): FilterOp {
  switch (type) {
    case "checkbox":
      return "checked";
    case "select":
      return "is";
    case "number":
      return "equals";
    case "date":
      return "after";
    default:
      return "contains";
  }
}

function matches(value: CellValue, f: Filter, type: Field["type"]): boolean {
  // Value-based ops are a no-op until the user supplies a value (so adding a
  // filter doesn't instantly hide every row).
  const valueless = f.op === "checked" || f.op === "unchecked";
  if (!valueless && (f.value == null || f.value === "")) return true;

  switch (f.op) {
    case "contains":
      return String(value ?? "")
        .toLowerCase()
        .includes(String(f.value ?? "").toLowerCase());
    case "equals":
      return type === "number"
        ? Number(value) === Number(f.value)
        : String(value ?? "") === String(f.value);
    case "is":
      return value === f.value;
    case "checked":
      return !!value;
    case "unchecked":
      return !value;
    case "gt":
      return value != null && Number(value) > Number(f.value);
    case "lt":
      return value != null && Number(value) < Number(f.value);
    case "before":
      return value != null && String(value) < String(f.value);
    case "after":
      return value != null && String(value) > String(f.value);
    default:
      return true;
  }
}

function compare(a: CellValue, b: CellValue, type: Field["type"]): number {
  if (a == null && b == null) return 0;
  if (a == null) return -1;
  if (b == null) return 1;
  if (type === "number") return Number(a) - Number(b);
  if (type === "checkbox") return (a ? 1 : 0) - (b ? 1 : 0);
  return String(a).localeCompare(String(b));
}

/** Apply a view's filters (AND) then sorts to a row set. */
export function applyView(
  rows: RowWithCells[],
  fields: Field[],
  filters: Filter[] = [],
  sorts: Sort[] = [],
): RowWithCells[] {
  const byId = new Map(fields.map((f) => [f.id, f]));
  let out = rows.filter((r) =>
    filters.every((f) => {
      const field = byId.get(f.fieldId);
      if (!field) return true;
      return matches(r.cells[f.fieldId], f, field.type);
    }),
  );
  for (const s of [...sorts].reverse()) {
    const field = byId.get(s.fieldId);
    if (!field) continue;
    out = [...out].sort((ra, rb) => {
      const c = compare(ra.cells[s.fieldId], rb.cells[s.fieldId], field.type);
      return s.dir === "asc" ? c : -c;
    });
  }
  return out;
}
