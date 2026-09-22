import jsep from "jsep";
import type {
  DatabaseBundle,
  Field,
  RowWithCells,
  CellValue,
} from "@/lib/api";

/** True for the field types whose value is derived, not stored/edited. */
export function isComputed(type: Field["type"]): boolean {
  return type === "lookup" || type === "rollup" || type === "formula";
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Node = any;

const FUNCS: Record<string, (...a: number[]) => number> = {
  round: (x, d = 0) => Math.round(x * 10 ** d) / 10 ** d,
  abs: Math.abs,
  floor: Math.floor,
  ceil: Math.ceil,
  min: Math.min,
  max: Math.max,
  sqrt: Math.sqrt,
};

const truthy = (v: CellValue) =>
  v !== 0 && v !== "" && v != null && v !== false;

function evalNode(node: Node, scope: Record<string, number | string>): CellValue {
  switch (node.type) {
    case "Literal":
      return node.value;
    case "Identifier":
      return scope[node.name] ?? 0;
    case "UnaryExpression": {
      const raw = evalNode(node.argument, scope);
      if (raw === null) return null;
      const v = Number(raw);
      return node.operator === "-" ? -v : node.operator === "!" ? (v ? 0 : 1) : v;
    }
    case "BinaryExpression": {
      const a = evalNode(node.left, scope);
      const b = evalNode(node.right, scope);
      // An undefined sub-expression (e.g. a divide by zero) poisons the whole
      // expression rather than silently becoming 0.
      if (a === null || b === null) return null;
      switch (node.operator) {
        case "+":
          return typeof a === "string" || typeof b === "string" ? `${a}${b}` : a + b;
        case "-":
          return Number(a) - Number(b);
        case "*":
          return Number(a) * Number(b);
        case "/":
          // Divide by zero has no sensible number: report it as empty ("–").
          return Number(b) === 0 ? null : Number(a) / Number(b);
        case "%":
          return Number(a) % Number(b);
        case ">":
          return a > b ? 1 : 0;
        case "<":
          return a < b ? 1 : 0;
        case ">=":
          return a >= b ? 1 : 0;
        case "<=":
          return a <= b ? 1 : 0;
        case "==":
          return a == b ? 1 : 0;
        case "!=":
          return a != b ? 1 : 0;
        // jsep 1.4 emits && / || as BinaryExpression (not LogicalExpression).
        case "&&":
          return truthy(a) && truthy(b) ? 1 : 0;
        case "||":
          return truthy(a) || truthy(b) ? 1 : 0;
        default:
          return 0;
      }
    }
    case "LogicalExpression": {
      // Defensive: some jsep configs emit this instead of BinaryExpression.
      const a = truthy(evalNode(node.left, scope));
      if (node.operator === "&&") return a && truthy(evalNode(node.right, scope)) ? 1 : 0;
      if (node.operator === "||") return a || truthy(evalNode(node.right, scope)) ? 1 : 0;
      return 0;
    }
    case "ConditionalExpression":
      return evalNode(node.test, scope)
        ? evalNode(node.consequent, scope)
        : evalNode(node.alternate, scope);
    case "CallExpression": {
      const fn = FUNCS[node.callee?.name];
      if (!fn) return 0;
      const raw = node.arguments.map((a: Node) => evalNode(a, scope));
      if (raw.length === 0 || raw.some((v: CellValue) => v === null)) return null;
      const out = fn(...raw.map((v: CellValue) => Number(v)));
      return isFinite(out) ? out : null;
    }
    default:
      return 0;
  }
}

/** Evaluate a formula expression that references fields as `{Field Name}`. */
export function evalFormula(
  expr: string,
  fields: Field[],
  cells: Record<string, CellValue>,
): CellValue {
  if (!expr) return null;
  let e = expr;
  const scope: Record<string, number | string> = {};
  fields.forEach((f, i) => {
    const token = `{${f.name}}`;
    if (e.includes(token)) {
      const raw = cells[f.id];
      const key = `v${i}`;
      scope[key] =
        typeof raw === "number"
          ? raw
          : typeof raw === "boolean"
            ? raw
              ? 1
              : 0
            : raw == null || raw === ""
              ? 0
              : isNaN(Number(raw))
                ? String(raw)
                : Number(raw);
      e = e.split(token).join(key);
    }
  });
  try {
    return evalNode(jsep(e), scope);
  } catch {
    return null;
  }
}

function linkedRows(
  relationValue: CellValue,
  targetBundle: DatabaseBundle | undefined,
): RowWithCells[] {
  if (!targetBundle) return [];
  const ids: string[] = Array.isArray(relationValue)
    ? relationValue
    : typeof relationValue === "string"
      ? safeArray(relationValue)
      : [];
  return ids
    .map((id) => targetBundle.rows.find((r) => r.id === id))
    .filter(Boolean) as RowWithCells[];
}

function safeArray(s: string): string[] {
  try {
    const p = JSON.parse(s);
    return Array.isArray(p) ? p : [];
  } catch {
    return [];
  }
}

/**
 * Aggregate a rollup's linked values. `count` and `sum` have a meaningful
 * answer over an empty set (0); `avg`, `min` and `max` do not, so they report
 * empty (null, rendered as "–") rather than a misleading 0.
 */
function aggregate(values: number[], fn: string): number | null {
  if (fn === "count") return values.length;
  if (fn === "sum") return values.reduce((a, b) => a + b, 0);
  if (values.length === 0) return null;
  switch (fn) {
    case "avg":
      return values.reduce((a, b) => a + b, 0) / values.length;
    case "min":
      return Math.min(...values);
    case "max":
      return Math.max(...values);
    default:
      return values.length;
  }
}

/**
 * Return the display value for a computed cell (lookup/rollup/formula).
 * `targetBundles` is keyed by database id, needed to resolve relations.
 */
export function computeCell(
  field: Field,
  row: RowWithCells,
  fields: Field[],
  targetBundles: Record<string, DatabaseBundle>,
  seen: Set<string> = new Set(),
): CellValue {
  if (seen.has(field.id)) return null; // guard against formula reference cycles
  const opts = field.options ?? {};
  if (field.type === "formula") {
    // A formula may reference other computed fields (lookups/rollups/formulas);
    // resolve those first and feed their values into the expression's scope.
    const next = new Set(seen).add(field.id);
    const cells: Record<string, CellValue> = { ...row.cells };
    for (const f of fields) {
      if (f.id !== field.id && isComputed(f.type)) {
        cells[f.id] = computeCell(f, row, fields, targetBundles, next);
      }
    }
    return evalFormula(opts.expr ?? "", fields, cells);
  }
  const relField = fields.find((f) => f.id === opts.relationFieldId);
  if (!relField) return null;
  const target = targetBundles[relField.options?.targetDatabaseId ?? ""];
  const rows = linkedRows(row.cells[relField.id], target);

  if (field.type === "lookup") {
    const vals = rows
      .map((r) => r.cells[opts.targetFieldId ?? ""])
      .filter((v) => v != null && v !== "");
    return vals.length <= 1 ? (vals[0] ?? null) : vals.join(", ");
  }
  if (field.type === "rollup") {
    // `count` = number of linked rows with a non-empty target value; it must not
    // depend on the value being numeric (unlike sum/avg/min/max).
    if ((opts.fn ?? "count") === "count") {
      return rows.filter((r) => {
        const v = r.cells[opts.targetFieldId ?? ""];
        return v != null && v !== "" && !(Array.isArray(v) && v.length === 0);
      }).length;
    }
    const nums = rows
      .map((r) => Number(r.cells[opts.targetFieldId ?? ""]))
      .filter((n) => !isNaN(n));
    return aggregate(nums, opts.fn ?? "sum");
  }
  return null;
}

/** Database ids referenced by this bundle's relation fields (to prefetch). */
export function relationTargetIds(bundle: DatabaseBundle): string[] {
  return [
    ...new Set(
      bundle.fields
        .filter((f) => f.type === "relation" && f.options?.targetDatabaseId)
        .map((f) => f.options!.targetDatabaseId!),
    ),
  ];
}
