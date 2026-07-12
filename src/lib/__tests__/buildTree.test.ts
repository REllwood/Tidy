import { describe, it, expect } from "vitest";
import { buildTree, type Page } from "@/lib/api";

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

describe("buildTree", () => {
  it("nests children under parents, sorted by position", () => {
    const pages = [
      mk("b", null, 2),
      mk("a", null, 1),
      mk("a2", "a", 2),
      mk("a1", "a", 1),
    ];
    const tree = buildTree(pages);
    expect(tree.map((n) => n.id)).toEqual(["a", "b"]); // roots sorted
    expect(tree[0].children.map((n) => n.id)).toEqual(["a1", "a2"]); // children sorted
  });

  it("treats pages with missing parents as roots", () => {
    const tree = buildTree([mk("orphan", "ghost", 1)]);
    expect(tree.map((n) => n.id)).toEqual(["orphan"]);
  });

  it("handles an empty list", () => {
    expect(buildTree([])).toEqual([]);
  });
});
