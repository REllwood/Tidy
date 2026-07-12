import { describe, it, expect } from "vitest";
import {
  simulate,
  seedPositions,
  adjacency,
  nodeRadius,
  type GraphNode,
  type GraphEdge,
} from "@/lib/graph";

const node = (id: string, degree = 0): GraphNode => ({
  id,
  title: id,
  icon: null,
  type: "doc",
  degree,
});

describe("seedPositions", () => {
  it("places every node inside the box", () => {
    const pos = seedPositions(["a", "b", "c"], 900, 600);
    expect(Object.keys(pos)).toHaveLength(3);
    for (const p of Object.values(pos)) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(900);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(600);
    }
  });
});

describe("simulate", () => {
  const nodes = [node("a", 1), node("b", 1), node("c", 0)];
  const edges: GraphEdge[] = [{ source: "a", target: "b" }];

  it("returns a position for every node, clamped to the box", () => {
    const pos = simulate(nodes, edges, { width: 900, height: 600, iterations: 100 });
    expect(Object.keys(pos).sort()).toEqual(["a", "b", "c"]);
    for (const p of Object.values(pos)) {
      expect(p.x).toBeGreaterThanOrEqual(20);
      expect(p.x).toBeLessThanOrEqual(880);
      expect(p.y).toBeGreaterThanOrEqual(20);
      expect(p.y).toBeLessThanOrEqual(580);
    }
  });

  it("is deterministic (no Math.random)", () => {
    const a = simulate(nodes, edges, { width: 900, height: 600, iterations: 80 });
    const b = simulate(nodes, edges, { width: 900, height: 600, iterations: 80 });
    expect(a).toEqual(b);
  });

  it("pulls linked nodes closer than unlinked ones", () => {
    const pos = simulate(nodes, edges, { width: 900, height: 600, iterations: 300 });
    const dist = (p: { x: number; y: number }, q: { x: number; y: number }) =>
      Math.hypot(p.x - q.x, p.y - q.y);
    // a–b are linked; a–c are not. The spring should keep a–b tighter than a–c.
    expect(dist(pos.a, pos.b)).toBeLessThan(dist(pos.a, pos.c));
  });

  it("centers a single node", () => {
    const pos = simulate([node("solo")], [], { width: 900, height: 600 });
    expect(Object.keys(pos)).toEqual(["solo"]);
    expect(pos.solo).toEqual({ x: 450, y: 300 });
  });

  it("ignores edges referencing unknown nodes", () => {
    const pos = simulate(nodes, [{ source: "a", target: "ghost" }], {
      iterations: 20,
    });
    expect(Object.keys(pos).sort()).toEqual(["a", "b", "c"]);
  });
});

describe("adjacency", () => {
  it("builds an undirected neighbor map", () => {
    const adj = adjacency([
      { source: "a", target: "b" },
      { source: "b", target: "c" },
    ]);
    expect([...adj.a]).toEqual(["b"]);
    expect([...adj.b].sort()).toEqual(["a", "c"]);
    expect([...adj.c]).toEqual(["b"]);
  });
});

describe("nodeRadius", () => {
  it("grows with degree and caps", () => {
    expect(nodeRadius(0)).toBe(6);
    expect(nodeRadius(3)).toBeGreaterThan(nodeRadius(1));
    expect(nodeRadius(100)).toBe(nodeRadius(8)); // capped at degree 8
  });
});
