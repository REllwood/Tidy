import type { RowWithCells } from "@/lib/api";

/** Column id used for cards with no value in the group-by field. */
export const NONE = "__none__";

/**
 * Positions are fractional so a card can be slotted between two neighbours.
 * Below this gap the midpoint stops being representable in a meaningful way,
 * so the whole database is renumbered 1..n instead.
 */
export const MIN_GAP = 1e-6;

export interface DropPlan {
  /** Where to move the dragged row to. */
  position: number;
  /** Rows to renumber first, when the neighbours have run out of room. */
  renormalise: { id: string; position: number }[] | null;
}

/** Which board column a row belongs to. */
export function columnOf(row: RowWithCells, groupFieldId: string): string {
  const v = row.cells[groupFieldId];
  return v == null || v === "" ? NONE : String(v);
}

/** Every row in a column, ordered by position. Pass the WHOLE row set. */
export function columnRows(
  allRows: RowWithCells[],
  groupFieldId: string,
  colId: string,
): RowWithCells[] {
  return allRows
    .filter((r) => columnOf(r, groupFieldId) === colId)
    .sort((a, b) => a.position - b.position);
}

/**
 * Append to the end of the whole database, not the end of the visible column:
 * positions are global, so using only the filtered rows lets a card land on top
 * of a hidden one.
 */
export function appendPosition(allRows: RowWithCells[]): number {
  return allRows.reduce((max, r) => Math.max(max, r.position), 0) + 1;
}

/** Renumber every row 1..n in its current order. */
export function renormalise(
  allRows: RowWithCells[],
): { id: string; position: number }[] {
  return [...allRows]
    .sort((a, b) => a.position - b.position)
    .map((r, i) => ({ id: r.id, position: i + 1 }));
}

function slotBefore(
  rows: RowWithCells[],
  groupFieldId: string,
  rowId: string,
  targetId: string,
): { position: number; ok: boolean } | null {
  const target = rows.find((r) => r.id === targetId);
  if (!target) return null;
  const colId = columnOf(target, groupFieldId);
  const colRows = columnRows(rows, groupFieldId, colId).filter(
    (r) => r.id !== rowId,
  );
  const idx = colRows.findIndex((r) => r.id === targetId);
  const prev = idx > 0 ? colRows[idx - 1] : undefined;
  if (!prev) return { position: target.position - 1, ok: true };
  const position = (prev.position + target.position) / 2;
  const ok =
    target.position - prev.position > MIN_GAP &&
    position > prev.position &&
    position < target.position;
  return { position, ok };
}

/**
 * Plan dropping `rowId` immediately before `targetId`, computed from the whole
 * row set so filtered-out rows can't be collided with.
 */
export function planDropBefore(
  allRows: RowWithCells[],
  groupFieldId: string,
  rowId: string,
  targetId: string,
): DropPlan | null {
  if (rowId === targetId) return null;
  const first = slotBefore(allRows, groupFieldId, rowId, targetId);
  if (!first) return null;
  if (first.ok) return { position: first.position, renormalise: null };

  // The neighbours are too close together: renumber everything, then slot into
  // the gap the renumbering just created.
  const plan = renormalise(allRows);
  const byId = new Map(plan.map((p) => [p.id, p.position]));
  const patched = allRows.map((r) => ({
    ...r,
    position: byId.get(r.id) ?? r.position,
  }));
  const second = slotBefore(patched, groupFieldId, rowId, targetId);
  if (!second) return null;
  return { position: second.position, renormalise: plan };
}

/** Plan dropping `rowId` at the end of a column. */
export function planDropAtEnd(allRows: RowWithCells[]): DropPlan {
  return { position: appendPosition(allRows), renormalise: null };
}
