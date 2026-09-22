import type { Page } from "@/lib/api";

/** Where a dragged page lands relative to the row it was dropped on. */
export type DropPlacement = "before" | "after" | "inside";

/** The arguments `move_page` needs, plus the row to open up after a nest. */
export interface MovePlan {
  id: string;
  parentId: string | null;
  position: number;
  /** Parent to expand so the moved page is visible where it landed. */
  expandId?: string;
}

const byPosition = (a: Page, b: Page) => a.position - b.position;

/** Is `id` the same page as `ancestorId`, or somewhere beneath it? */
export function isSelfOrDescendant(
  pages: Page[],
  id: string,
  ancestorId: string,
): boolean {
  const byId = new Map(pages.map((p) => [p.id, p]));
  const seen = new Set<string>();
  let cur: Page | undefined = byId.get(id);
  while (cur && !seen.has(cur.id)) {
    if (cur.id === ancestorId) return true;
    seen.add(cur.id);
    cur = cur.parent_id ? byId.get(cur.parent_id) : undefined;
  }
  return false;
}

/** The pages under `parentId`, in display order, ignoring the dragged page. */
function siblingsOf(
  pages: Page[],
  parentId: string | null,
  excludeId: string,
): Page[] {
  return pages
    .filter(
      (p) =>
        (p.parent_id ?? null) === parentId &&
        p.id !== excludeId,
    )
    .sort(byPosition);
}

const nextPosition = (siblings: Page[]) =>
  (siblings.length ? Math.max(...siblings.map((s) => s.position)) : 0) + 1;

/**
 * Work out the move for dropping `activeId` on `targetId`.
 *
 * Reordering uses the midpoint between the two neighbours it lands between, so
 * a drop only ever rewrites the dragged page's own position. Returns null for
 * a no-op or a move that would put a page inside its own subtree.
 */
export function planMove(
  pages: Page[],
  activeId: string,
  targetId: string,
  placement: DropPlacement,
): MovePlan | null {
  if (activeId === targetId) return null;
  const target = pages.find((p) => p.id === targetId);
  if (!target) return null;
  // Never drop a page into itself or one of its descendants.
  if (isSelfOrDescendant(pages, targetId, activeId)) return null;

  if (placement === "inside") {
    return {
      id: activeId,
      parentId: targetId,
      position: nextPosition(siblingsOf(pages, targetId, activeId)),
      expandId: targetId,
    };
  }

  const parentId = target.parent_id ?? null;
  const siblings = siblingsOf(pages, parentId, activeId);
  const index = siblings.findIndex((p) => p.id === targetId);
  const neighbour =
    placement === "before" ? siblings[index - 1] : siblings[index + 1];
  const position = neighbour
    ? (neighbour.position + target.position) / 2
    : placement === "before"
      ? target.position - 1
      : target.position + 1;

  return {
    id: activeId,
    parentId,
    position,
    expandId: parentId ?? undefined,
  };
}

/** Drop on the empty space below the tree: move back out to the top level. */
export function planMoveToRoot(
  pages: Page[],
  activeId: string,
): MovePlan | null {
  const active = pages.find((p) => p.id === activeId);
  if (!active) return null;
  if ((active.parent_id ?? null) === null) return null; // already at the root
  return {
    id: activeId,
    parentId: null,
    position: nextPosition(siblingsOf(pages, null, activeId)),
  };
}

// ---- drop-zone ids --------------------------------------------------------

/** The zone below the tree that moves a page back out to the top level. */
export const ROOT_DROP_ID = "tree-root";

/** Droppable id for one of a row's three zones. */
export const dropId = (placement: DropPlacement, pageId: string) =>
  `${placement}:${pageId}`;

/** Read a droppable id back, or null when it isn't one of ours. */
export function parseDropId(
  id: string,
): { placement: DropPlacement; pageId: string } | null {
  const at = id.indexOf(":");
  if (at < 1) return null;
  const placement = id.slice(0, at);
  const pageId = id.slice(at + 1);
  if (!pageId) return null;
  if (placement !== "before" && placement !== "after" && placement !== "inside")
    return null;
  return { placement, pageId };
}
