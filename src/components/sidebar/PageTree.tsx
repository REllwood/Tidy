import { useCallback, useMemo, useRef, useState } from "react";
import {
  DndContext,
  PointerSensor,
  KeyboardSensor,
  useDroppable,
  useSensor,
  useSensors,
  pointerWithin,
  rectIntersection,
  type CollisionDetection,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { CornerLeftUp } from "lucide-react";
import type { PageNode } from "@/lib/api";
import { useUi } from "@/store/ui";
import {
  usePages,
  useMovePage,
  useRenamePage,
  useSetFavorite,
  useDeletePage,
  useCreatePage,
} from "@/hooks/usePages";
import { PageTreeItem } from "./PageTreeItem";
import { TreeActionsProvider, type TreeActions } from "./treeActions";
import {
  isSelfOrDescendant,
  parseDropId,
  planMove,
  planMoveToRoot,
  ROOT_DROP_ID,
} from "./treeMove";

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

/** Pointer position decides the zone; keyboard dragging falls back to rects. */
const collisionDetection: CollisionDetection = (args) => {
  const byPointer = pointerWithin(args);
  return byPointer.length ? byPointer : rectIntersection(args);
};

const isTypingTarget = (el: EventTarget | null) => {
  const node = el as HTMLElement | null;
  return (
    !!node &&
    (node.tagName === "INPUT" ||
      node.tagName === "TEXTAREA" ||
      node.isContentEditable)
  );
};

/**
 * Enter and Space open the focused page (see onKeyDown), so a keyboard move
 * starts with Shift+Space instead of hijacking them.
 */
class TreeKeyboardSensor extends KeyboardSensor {
  static activators: typeof KeyboardSensor.activators = [
    {
      eventName: "onKeyDown",
      handler: (event, options, context) =>
        event.shiftKey &&
        KeyboardSensor.activators[0].handler(
          event,
          {
            ...options,
            keyboardCodes: {
              start: ["Space"],
              cancel: ["Escape"],
              end: ["Space", "Enter"],
            },
          },
          context,
        ),
    },
  ];
}

export function PageTree({ tree }: { tree: PageNode[] }) {
  const setExpanded = useUi((s) => s.setExpanded);
  const openPage = useUi((s) => s.openPage);
  const { data: pages } = usePages();
  const move = useMovePage();
  const rename = useRenamePage();
  const favorite = useSetFavorite();
  const del = useDeletePage();
  const create = useCreatePage();
  const refs = useRef(new Map<string, HTMLDivElement>());
  const [draggingId, setDraggingId] = useState<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(TreeKeyboardSensor),
  );

  // `mutate` is stable, so these callbacks are too and the rows stay memoised.
  const renameMutate = rename.mutate;
  const favoriteMutate = favorite.mutate;
  const deleteMutate = del.mutate;
  const createMutate = create.mutate;
  const actions = useMemo<TreeActions>(
    () => ({
      rename: (id, title) => renameMutate({ id, title }),
      setFavourite: (id, isFavorite) => favoriteMutate({ id, isFavorite }),
      remove: (id) => deleteMutate(id),
      addSubpage: (parentId) => {
        setExpanded(parentId, true);
        // Open the new page so it can be named straight away.
        createMutate(
          { parentId, title: "Untitled" },
          { onSuccess: (page) => openPage(page.id) },
        );
      },
      canDrop: (activeId, targetId) =>
        !!activeId &&
        !!pages &&
        activeId !== targetId &&
        !isSelfOrDescendant(pages, targetId, activeId),
    }),
    [
      renameMutate,
      favoriteMutate,
      deleteMutate,
      createMutate,
      setExpanded,
      openPage,
      pages,
    ],
  );

  const onDragStart = (e: DragStartEvent) => setDraggingId(String(e.active.id));

  const onDragEnd = (e: DragEndEvent) => {
    setDraggingId(null);
    const activeId = String(e.active.id);
    const overId = e.over ? String(e.over.id) : null;
    if (!overId || !pages) return;

    let plan = null;
    if (overId === ROOT_DROP_ID) {
      plan = planMoveToRoot(pages, activeId);
    } else {
      const zone = parseDropId(overId);
      if (zone) plan = planMove(pages, activeId, zone.pageId, zone.placement);
    }
    if (!plan) return;

    if (plan.expandId) setExpanded(plan.expandId, true);
    move.mutate({
      id: plan.id,
      parentId: plan.parentId,
      position: plan.position,
    });
  };

  const registerRef = useCallback((id: string, el: HTMLDivElement | null) => {
    if (el) refs.current.set(id, el);
    else refs.current.delete(id);
  }, []);

  const focusId = (id: string) => {
    refs.current.get(id)?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    // Never hijack typing (the rename field lives inside a row).
    if (isTypingTarget(e.target)) return;
    const expanded = useUi.getState().expanded;
    const visible = flattenVisible(tree, expanded);
    if (!visible.length) return;
    const activeEl = document.activeElement as HTMLElement | null;
    const curId = activeEl?.getAttribute("data-tree-id");
    const idx = visible.findIndex((n) => n.id === curId);

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusId(
          visible[Math.min(visible.length - 1, idx + 1)]?.id ?? visible[0].id,
        );
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
        // Shift+Space picks the page up to move it (TreeKeyboardSensor).
        if (e.shiftKey) break;
        e.preventDefault();
        if (curId) openPage(curId);
        break;
    }
  };

  if (!tree.length) {
    return <div className="px-2 py-1 text-sm text-text-faint">No pages yet</div>;
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={collisionDetection}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragCancel={() => setDraggingId(null)}
    >
      <TreeActionsProvider value={actions}>
        <div
          role="tree"
          aria-label="Pages"
          onKeyDown={onKeyDown}
          onFocus={(e) => {
            // make the first item focusable when the tree gains focus
            if (e.target === e.currentTarget) {
              const first = flattenVisible(tree, useUi.getState().expanded)[0];
              if (first) focusId(first.id);
            }
          }}
          tabIndex={0}
          className="rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
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
        <RootDropZone active={!!draggingId} />
      </TreeActionsProvider>
    </DndContext>
  );
}

/** Drop target below the tree: takes a nested page back out to the top level. */
function RootDropZone({ active }: { active: boolean }) {
  const { isOver, setNodeRef } = useDroppable({ id: ROOT_DROP_ID });
  return (
    <div
      ref={setNodeRef}
      aria-hidden={!active}
      className={`mt-1 flex items-center gap-1.5 overflow-hidden rounded-md border border-dashed px-2 text-2xs transition-all duration-150 ${
        active
          ? "h-8 opacity-100"
          : "pointer-events-none h-0 border-transparent opacity-0"
      } ${
        isOver
          ? "border-brand bg-brand-soft text-text"
          : "border-border-strong text-text-faint"
      }`}
    >
      <CornerLeftUp className="size-3" />
      Move to the top level
    </div>
  );
}
