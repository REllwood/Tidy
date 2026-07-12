/**
 * Dependency-free force-directed graph layout (Fruchterman–Reingold).
 * Deterministic: initial positions are seeded on a circle by node index (no
 * Math.random), so the same graph always lays out the same way, which also
 * makes it unit-testable.
 */

export interface GraphNode {
  id: string;
  title: string;
  icon: string | null;
  type: string;
  degree: number;
}
export interface GraphEdge {
  source: string;
  target: string;
}
export interface LinkGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface Vec2 {
  x: number;
  y: number;
}
export type Positions = Record<string, Vec2>;

export interface LayoutOptions {
  width?: number;
  height?: number;
  iterations?: number;
}

/** Deterministic starting ring so disconnected nodes still spread out. */
export function seedPositions(
  ids: string[],
  width: number,
  height: number,
): Positions {
  const cx = width / 2;
  const cy = height / 2;
  const r = Math.min(width, height) * 0.35;
  const pos: Positions = {};
  if (ids.length === 1) {
    pos[ids[0]] = { x: cx, y: cy };
    return pos;
  }
  const n = Math.max(ids.length, 1);
  ids.forEach((id, i) => {
    const angle = (2 * Math.PI * i) / n;
    pos[id] = { x: cx + r * Math.cos(angle), y: cy + r * Math.sin(angle) };
  });
  return pos;
}

/**
 * Run the FR simulation to a stable-ish layout and return final positions,
 * clamped to the [0,width]×[0,height] box.
 */
export function simulate(
  nodes: GraphNode[],
  edges: GraphEdge[],
  opts: LayoutOptions = {},
): Positions {
  const width = opts.width ?? 900;
  const height = opts.height ?? 600;
  // Repulsion is O(n²) per iteration; scale the pass count down for large
  // graphs so a big vault can't freeze the layout for seconds.
  const iterations =
    opts.iterations ??
    (nodes.length > 200 ? 80 : nodes.length > 80 ? 150 : 300);

  const ids = nodes.map((n) => n.id);
  const idSet = new Set(ids);
  const pos = seedPositions(ids, width, height);
  if (nodes.length <= 1) return pos;

  // ideal edge length
  const area = width * height;
  const k = 0.8 * Math.sqrt(area / nodes.length);
  const k2 = k * k;

  // only keep edges whose endpoints are real nodes
  const validEdges = edges.filter(
    (e) => idSet.has(e.source) && idSet.has(e.target) && e.source !== e.target,
  );

  let temp = width / 10;
  const cool = temp / (iterations + 1);
  const cx = width / 2;
  const cy = height / 2;

  for (let iter = 0; iter < iterations; iter++) {
    const disp: Positions = {};
    for (const id of ids) disp[id] = { x: 0, y: 0 };

    // repulsion, every pair pushes apart
    for (let i = 0; i < ids.length; i++) {
      for (let j = i + 1; j < ids.length; j++) {
        const a = pos[ids[i]];
        const b = pos[ids[j]];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        let dist = Math.hypot(dx, dy);
        if (dist < 0.01) {
          // deterministic nudge for coincident nodes
          dx = (i - j) * 0.01 + 0.01;
          dy = 0.01;
          dist = Math.hypot(dx, dy);
        }
        const force = k2 / dist;
        const ux = dx / dist;
        const uy = dy / dist;
        disp[ids[i]].x += ux * force;
        disp[ids[i]].y += uy * force;
        disp[ids[j]].x -= ux * force;
        disp[ids[j]].y -= uy * force;
      }
    }

    // attraction, edges pull together
    for (const e of validEdges) {
      const a = pos[e.source];
      const b = pos[e.target];
      let dx = a.x - b.x;
      let dy = a.y - b.y;
      const dist = Math.hypot(dx, dy) || 0.01;
      const force = (dist * dist) / k;
      const ux = dx / dist;
      const uy = dy / dist;
      disp[e.source].x -= ux * force;
      disp[e.source].y -= uy * force;
      disp[e.target].x += ux * force;
      disp[e.target].y += uy * force;
    }

    // gentle gravity toward center so components don't drift off-canvas
    for (const id of ids) {
      disp[id].x += (cx - pos[id].x) * 0.01;
      disp[id].y += (cy - pos[id].y) * 0.01;
    }

    // apply displacement capped by temperature, then clamp to box
    for (const id of ids) {
      const d = disp[id];
      const len = Math.hypot(d.x, d.y) || 0.01;
      const step = Math.min(len, temp);
      pos[id].x += (d.x / len) * step;
      pos[id].y += (d.y / len) * step;
      pos[id].x = Math.max(20, Math.min(width - 20, pos[id].x));
      pos[id].y = Math.max(20, Math.min(height - 20, pos[id].y));
    }
    temp = Math.max(temp - cool, 1);
  }

  return pos;
}

/** Neighbor adjacency (undirected) for hover highlighting. */
export function adjacency(edges: GraphEdge[]): Record<string, Set<string>> {
  const adj: Record<string, Set<string>> = {};
  const add = (a: string, b: string) => {
    (adj[a] ??= new Set()).add(b);
  };
  for (const e of edges) {
    add(e.source, e.target);
    add(e.target, e.source);
  }
  return adj;
}

/** Radius for a node, scaled by its degree. */
export function nodeRadius(degree: number): number {
  return 6 + Math.min(degree, 8) * 1.6;
}
