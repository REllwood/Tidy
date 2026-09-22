import { describe, it, expect } from "vitest";
import {
  isSelfOrDescendant,
  planMove,
  planMoveToRoot,
} from "@/components/sidebar/treeMove";
import type { Page } from "@/lib/api";

const mk = (id: string, parent_id: string | null, position: number): Page => ({
  id,
  parent_id,
  title: id,
  icon: null,
  type: "doc",
  position,
  is_favorite: false,
  created_at: 0,
  updated_at: 0,
});

// a (1) ─ a1 (1), a2 (2)
// b (2)
// c (3)
const pages: Page[] = [
  mk("a", null, 1),
  mk("b", null, 2),
  mk("c", null, 3),
  mk("a1", "a", 1),
  mk("a2", "a", 2),
];

describe("planMove: reordering among siblings", () => {
  it("drops between two neighbours at their midpoint", () => {
    expect(planMove(pages, "a", "c", "before")).toEqual({
      id: "a",
      parentId: null,
      position: 2.5, // between b (2) and c (3)
      expandId: undefined,
    });
  });

  it("drops above the first sibling without touching the others", () => {
    expect(planMove(pages, "c", "a", "before")?.position).toBe(0);
  });

  it("drops below the last sibling", () => {
    expect(planMove(pages, "a", "c", "after")?.position).toBe(4);
  });

  it("ignores the dragged page when measuring the gap", () => {
    // b is being dragged, so the gap "after a" runs a (1) → c (3)
    expect(planMove(pages, "b", "a", "after")?.position).toBe(2);
  });

  it("re-parents when the target sits under another page", () => {
    expect(planMove(pages, "b", "a2", "before")).toEqual({
      id: "b",
      parentId: "a",
      position: 1.5,
      expandId: "a",
    });
  });
});

describe("planMove: nesting", () => {
  it("appends after the existing children and asks for the parent to open", () => {
    expect(planMove(pages, "b", "a", "inside")).toEqual({
      id: "b",
      parentId: "a",
      position: 3, // after a2 (2)
      expandId: "a",
    });
  });

  it("nests into a page that has no children yet", () => {
    expect(planMove(pages, "a1", "c", "inside")?.position).toBe(1);
  });
});

describe("planMove: guards", () => {
  it("refuses a drop on itself", () => {
    expect(planMove(pages, "a", "a", "inside")).toBeNull();
    expect(planMove(pages, "a", "a", "before")).toBeNull();
  });

  it("refuses a drop inside its own subtree (no cycles)", () => {
    expect(planMove(pages, "a", "a1", "inside")).toBeNull();
    expect(planMove(pages, "a", "a2", "after")).toBeNull();
  });

  it("refuses an unknown target", () => {
    expect(planMove(pages, "a", "ghost", "inside")).toBeNull();
  });

  it("survives a corrupt parent cycle without hanging", () => {
    const looped = [mk("x", "y", 1), mk("y", "x", 1)];
    expect(isSelfOrDescendant(looped, "x", "z")).toBe(false);
  });
});

describe("planMoveToRoot", () => {
  it("moves a nested page out to the end of the top level", () => {
    expect(planMoveToRoot(pages, "a1")).toEqual({
      id: "a1",
      parentId: null,
      position: 4, // after c (3)
    });
  });

  it("is a no-op for a page already at the top level", () => {
    expect(planMoveToRoot(pages, "b")).toBeNull();
  });
});
