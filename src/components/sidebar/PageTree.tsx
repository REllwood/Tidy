import { useCallback, useRef } from "react";
import {
  DndContext,
  PointerSensor,
  KeyboardSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import type { PageNode } from "@/lib/api";
import { useUi } from "@/store/ui";
import { usePages, useMovePage } from "@/hooks/usePages";
import { PageTreeItem } from "./PageTreeItem";

/** Collect a node id and all its descendant ids (to block invalid drops). */
function descendantIds(
  nodes: PageNode[],
  rootId: string,
): Set<string> {
  const out = new Set<string>();
  const walk = (n: PageNode, inside: boolean) => {
    if (inside) out.add(n.id);
    for (const c of n.children) walk(c, inside || n.id === rootId);
  };
  for (const n of nodes) walk(n, n.id === rootId);
  out.add(rootId);
  return out;
}

/** Flatten the visible (expanded) nodes in display order for keyboard nav. */
function flattenVisible(
  nodes: PageNode[],
  expanded: Record<string, boolean>,
  out: PageNode[] = [],
): PageNode[] {
  for (const n of nodes) {
    out.push(n);
    if (n.children.length && expanded[n.id]) {
      flattenVisible(n.children, expanded, out);
    }
  }
  return out;
}

export function PageTree({ tree }: { tree: PageNode[] }) {
  const expanded = useUi((s) => s.expanded);
  const setExpanded = useUi((s) => s.setExpanded);
  const openPage = useUi((s) => s.openPage);
  const { data: pages } = usePages();
  const move = useMovePage();
  const refs = useRef(new Map<string, HTMLDivElement>());

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor),
  );

  const onDragEnd = (e: DragEndEvent) => {
    const activeId = String(e.active.id);
    const overId = e.over ? String(e.over.id) : null;
    if (!overId || overId === activeId || !pages) return;
    // Don't drop a page into itself or one of its descendants.
    if (descendantIds(tree, activeId).has(overId)) return;
    // Nest the dragged page as the last child of the drop target.
    const childPositions = pages
      .filter((p) => p.parent_id === overId)
      .map((p) => p.position);
    const position = (childPositions.length ? Math.max(...childPositions) : 0) + 1;
    setExpanded(overId, true);
    move.mutate({ id: activeId, parentId: overId, position });
  };

  const registerRef = useCallback((id: string, el: HTMLDivElement | null) => {
    if (el) refs.current.set(id, el);
    else refs.current.delete(id);
  }, []);

  const focusId = (id: string) => {
    const el = refs.current.get(id);
    el?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const visible = flattenVisible(tree, expanded);
    if (!visible.length) return;
    const activeEl = document.activeElement as HTMLElement | null;
    const curId = activeEl?.getAttribute("data-tree-id");
    const idx = visible.findIndex((n) => n.id === curId);

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusId(visible[Math.min(visible.length - 1, idx + 1)]?.id ?? visible[0].id);
        break;
      case "ArrowUp":
        e.preventDefault();
        focusId(visible[Math.max(0, idx - 1)]?.id ?? visible[0].id);
        break;
      case "ArrowRight": {
        e.preventDefault();
        const n = visible[idx];
        if (n && n.children.length) {
          if (!expanded[n.id]) setExpanded(n.id, true);
          else focusId(n.children[0].id);
        }
        break;
      }
      case "ArrowLeft": {
        e.preventDefault();
        const n = visible[idx];
        if (n && n.children.length && expanded[n.id]) setExpanded(n.id, false);
        else if (n?.parent_id) focusId(n.parent_id);
        break;
      }
      case "Enter":
      case " ":
        e.preventDefault();
        if (curId) openPage(curId);
        break;
    }
  };

  if (!tree.length) {
    return (
      <div className="px-2 py-1 text-sm text-text-faint">No pages yet</div>
    );
  }

  return (
    <DndContext sensors={sensors} onDragEnd={onDragEnd}>
      <div
        role="tree"
        aria-label="Pages"
        onKeyDown={onKeyDown}
        onFocus={(e) => {
          // make the first item focusable when the tree gains focus
          if (e.target === e.currentTarget) {
            const first = flattenVisible(tree, expanded)[0];
            if (first) focusId(first.id);
          }
        }}
        tabIndex={0}
        className="outline-none"
      >
        {tree.map((node) => (
          <PageTreeItem
            key={node.id}
            node={node}
            depth={0}
            registerRef={registerRef}
          />
        ))}
      </div>
    </DndContext>
  );
}
