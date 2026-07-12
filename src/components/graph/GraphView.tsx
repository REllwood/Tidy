import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Waypoints } from "lucide-react";
import { knowledgeApi } from "@/lib/api";
import { useUi } from "@/store/ui";
import {
  simulate,
  seedPositions,
  adjacency,
  nodeRadius,
  type Positions,
} from "@/lib/graph";
import { Skeleton } from "@/components/ui/skeleton";

const W = 900;
const H = 600;

// Node fill by page kind, tied to the app palette via CSS variables.
const KIND_COLOR: Record<string, string> = {
  database: "var(--brand)",
  record: "var(--info)",
  doc: "var(--text-muted)",
};

export function GraphView() {
  const openPage = useUi((s) => s.openPage);
  const [hover, setHover] = useState<string | null>(null);

  const { data, isLoading, isError } = useQuery({
    queryKey: ["graph"],
    queryFn: () => knowledgeApi.getGraph(),
  });

  // Seed instantly (cheap) for first paint, then run the O(n²) force layout off
  // the render path in an effect so a large graph never blocks the UI.
  const [positions, setPositions] = useState<Positions>({});
  useEffect(() => {
    if (!data) return;
    setPositions(seedPositions(data.nodes.map((n) => n.id), W, H));
    let cancelled = false;
    const handle = setTimeout(() => {
      const laid = simulate(data.nodes, data.edges, { width: W, height: H });
      if (!cancelled) setPositions(laid);
    }, 0);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [data]);
  const adj = useMemo(() => (data ? adjacency(data.edges) : {}), [data]);

  if (isLoading) {
    return (
      <div className="grid h-full place-items-center p-6">
        <Skeleton className="h-[60%] w-[70%]" />
      </div>
    );
  }
  if (isError || !data) {
    return <div className="p-6 text-danger-c">Couldn't load the graph.</div>;
  }

  const { nodes, edges } = data;

  if (nodes.length === 0) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <div className="max-w-sm">
          <Waypoints className="mx-auto mb-3 size-8 text-text-faint" />
          <h2 className="text-lg font-semibold">Your graph is empty</h2>
          <p className="mt-1 text-sm text-text-muted">
            Link pages with <code>[[wiki-links]]</code> and they'll appear here
            as a connected map.
          </p>
        </div>
      </div>
    );
  }

  const isDimmed = (id: string) =>
    hover !== null && hover !== id && !adj[hover]?.has(id);
  const edgeActive = (s: string, t: string) =>
    hover !== null && (hover === s || hover === t);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border px-4 py-2.5">
        <Waypoints className="size-4 text-text-muted" />
        <h1 className="text-sm font-semibold">Graph view</h1>
        <span className="text-xs text-text-faint">
          {nodes.length} pages · {edges.length} links
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden bg-bg">
        <svg
          viewBox={`0 0 ${W} ${H}`}
          preserveAspectRatio="xMidYMid meet"
          className="h-full w-full"
          role="img"
          aria-label="Knowledge graph"
        >
          {/* edges */}
          <g>
            {edges.map((e, i) => {
              const a = positions[e.source];
              const b = positions[e.target];
              if (!a || !b) return null;
              const active = edgeActive(e.source, e.target);
              const dim = hover !== null && !active;
              return (
                <line
                  key={i}
                  x1={a.x}
                  y1={a.y}
                  x2={b.x}
                  y2={b.y}
                  stroke="var(--border)"
                  strokeWidth={active ? 2 : 1}
                  strokeOpacity={dim ? 0.2 : active ? 0.9 : 0.55}
                />
              );
            })}
          </g>

          {/* nodes */}
          <g>
            {nodes.map((n) => {
              const p = positions[n.id];
              if (!p) return null;
              const r = nodeRadius(n.degree);
              const dim = isDimmed(n.id);
              const active = hover === n.id;
              return (
                <g
                  key={n.id}
                  transform={`translate(${p.x},${p.y})`}
                  opacity={dim ? 0.28 : 1}
                  className="cursor-pointer"
                  onMouseEnter={() => setHover(n.id)}
                  onMouseLeave={() => setHover(null)}
                  onClick={() => openPage(n.id)}
                >
                  <circle
                    r={r}
                    fill={KIND_COLOR[n.type] ?? KIND_COLOR.doc}
                    stroke="var(--bg)"
                    strokeWidth={2}
                  />
                  <text
                    y={r + 12}
                    textAnchor="middle"
                    className="select-none"
                    fontSize={active ? 13 : 11}
                    fontWeight={active ? 600 : 400}
                    fill="var(--text)"
                  >
                    {(n.icon ? n.icon + " " : "") +
                      truncate(n.title || "Untitled", 22)}
                  </text>
                </g>
              );
            })}
          </g>
        </svg>
      </div>

      <div className="flex items-center gap-4 border-t border-border px-4 py-2 text-xs text-text-faint">
        <Legend color={KIND_COLOR.doc} label="Doc" />
        <Legend color={KIND_COLOR.database} label="Database" />
        <Legend color={KIND_COLOR.record} label="Record" />
        <span className="ml-auto">Hover to focus · click to open</span>
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <span
        className="inline-block size-2.5 rounded-full"
        style={{ background: color }}
      />
      {label}
    </span>
  );
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}
