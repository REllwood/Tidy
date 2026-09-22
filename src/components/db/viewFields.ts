import type {
  CellValue,
  DatabaseBundle,
  Field,
  RowWithCells,
  ViewConfig,
} from "@/lib/api";
import { computeCell, isComputed } from "@/lib/computed";

/** Relation cells hold an array of row ids (sometimes JSON-encoded). */
export function asIds(value: CellValue): string[] {
  if (Array.isArray(value)) return value as string[];
  if (typeof value === "string" && value) {
    try {
      const p = JSON.parse(value);
      return Array.isArray(p) ? p : [];
    } catch {
      return [];
    }
  }
  return [];
}

/** Human label for a target row: its primary (first) field's value. */
export function rowLabel(
  target: DatabaseBundle | undefined,
  row: RowWithCells,
): string {
  const primary = target?.fields[0];
  const v = primary ? row.cells[primary.id] : undefined;
  return v == null || v === "" ? "Untitled" : String(v);
}

/**
 * A view config stores field ids. Fields can be deleted underneath it, so every
 * view resolves its config through here: it falls back to the first field of
 * the right type and reports when the configured field has gone, so the view
 * can show a notice instead of an empty screen.
 */
export interface ResolvedField {
  field: Field | undefined;
  /** The config named a field that no longer exists on this database. */
  missing: boolean;
}

export function resolveViewField(
  fields: Field[],
  configId: string | undefined,
  accepts: (f: Field) => boolean,
  fallback = true,
): ResolvedField {
  const named = configId ? fields.find((f) => f.id === configId) : undefined;
  if (named && accepts(named)) return { field: named, missing: false };
  const missing = !!configId && !named;
  if (named && !accepts(named)) return { field: undefined, missing: false };
  return {
    field: fallback ? fields.find(accepts) : undefined,
    missing,
  };
}

const isType = (type: Field["type"]) => (f: Field) => f.type === type;

/** Board columns come from a select field. */
export function boardGroupField(
  fields: Field[],
  cfg: ViewConfig | null | undefined,
): ResolvedField {
  return resolveViewField(fields, cfg?.groupByFieldId, isType("select"));
}

/** Calendar events come from a date field. */
export function calendarDateField(
  fields: Field[],
  cfg: ViewConfig | null | undefined,
): ResolvedField {
  return resolveViewField(fields, cfg?.dateFieldId, isType("date"));
}

export interface GanttFields {
  start: ResolvedField;
  end: ResolvedField;
  deps: ResolvedField;
}

/** Gantt bars need a start date; end and dependencies are optional. */
export function ganttFields(
  fields: Field[],
  cfg: ViewConfig | null | undefined,
): GanttFields {
  return {
    start: resolveViewField(fields, cfg?.startFieldId, isType("date")),
    end: resolveViewField(fields, cfg?.endFieldId, isType("date"), false),
    deps: resolveViewField(
      fields,
      cfg?.dependenciesFieldId,
      isType("dependencies"),
    ),
  };
}

/** The field a card/bar/event is titled by. */
export function titleField(fields: Field[]): Field | undefined {
  return fields.find((f) => f.type === "text") ?? fields[0];
}

/** Drop filters and sorts that point at fields which no longer exist. */
export function pruneViewConfig(
  cfg: ViewConfig | null | undefined,
  fields: Field[],
): ViewConfig {
  const live = new Set(fields.map((f) => f.id));
  const next: ViewConfig = { ...(cfg ?? {}) };
  if (next.filters) next.filters = next.filters.filter((f) => live.has(f.fieldId));
  if (next.sorts) next.sorts = next.sorts.filter((s) => live.has(s.fieldId));
  if (next.columnWidths)
    next.columnWidths = Object.fromEntries(
      Object.entries(next.columnWidths).filter(([id]) => live.has(id)),
    );
  for (const k of [
    "groupByFieldId",
    "dateFieldId",
    "startFieldId",
    "endFieldId",
    "dependenciesFieldId",
  ] as const) {
    if (next[k] && !live.has(next[k]!)) delete next[k];
  }
  return next;
}

/**
 * The value a filter, sort or card should see for a cell: computed columns are
 * evaluated, and relations read as the linked rows' names.
 */
export function resolveCellValue(
  row: RowWithCells,
  field: Field,
  fields: Field[],
  targetBundles: Record<string, DatabaseBundle>,
): CellValue {
  if (isComputed(field.type))
    return computeCell(field, row, fields, targetBundles);
  if (field.type === "relation") {
    const target = targetBundles[field.options?.targetDatabaseId ?? ""];
    const ids = asIds(row.cells[field.id]);
    if (!target) return ids;
    return ids.map((id) => {
      const r = target.rows.find((x) => x.id === id);
      return r ? rowLabel(target, r) : id;
    });
  }
  return row.cells[field.id];
}

/**
 * Short text for a link cell on a card: the linked rows' names, or a count
 * while the related database is still loading (never raw row ids).
 */
export function linkText(
  row: RowWithCells,
  field: Field,
  targetBundles: Record<string, DatabaseBundle>,
): string {
  const ids = asIds(row.cells[field.id]);
  if (!ids.length) return "";
  const target =
    field.type === "relation"
      ? targetBundles[field.options?.targetDatabaseId ?? ""]
      : undefined;
  if (!target) return `${ids.length} linked`;
  const names = ids
    .map((id) => {
      const r = target.rows.find((x) => x.id === id);
      return r ? rowLabel(target, r) : null;
    })
    .filter(Boolean) as string[];
  return names.length ? names.join(", ") : `${ids.length} linked`;
}
