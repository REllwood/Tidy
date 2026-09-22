import { Fragment, memo, useCallback, useEffect, useRef, useState } from "react";
import { useDraggable, useDroppable } from "@dnd-kit/core";
import {
  ChevronRight,
  FileText,
  Database,
  Plus,
  MoreHorizontal,
} from "lucide-react";
import type { PageNode } from "@/lib/api";
import { useMergedRefs } from "@/lib/utils";
import { useUi } from "@/store/ui";
import { useTreeActions } from "./treeActions";
import { dropId } from "./treeMove";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

/** Indent per level, clamped so a deep branch can't push titles off-screen. */
const indentFor = (depth: number) => 8 + Math.min(depth, 6) * 14;

export const PageTreeItem = memo(function PageTreeItem({
  node,
  depth,
  registerRef,
}: {
  node: PageNode;
  depth: number;
  registerRef: (id: string, el: HTMLDivElement | null) => void;
}) {
  const expanded = useUi((s) => s.expanded[node.id] ?? false);
  const toggleExpanded = useUi((s) => s.toggleExpanded);
  const openPage = useUi((s) => s.openPage);
  const isActive = useUi(
    (s) => s.activePane.kind === "page" && s.activePane.pageId === node.id,
  );
  const actions = useTreeActions();

  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(node.title);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const hasChildren = node.children.length > 0;

  const drag = useDraggable({ id: node.id });
  const nest = useDroppable({ id: dropId("inside", node.id) });
  const before = useDroppable({ id: dropId("before", node.id) });
  const after = useDroppable({ id: dropId("after", node.id) });

  const isDragging = drag.isDragging;
  const draggedId = drag.active ? String(drag.active.id) : null;
  const droppable = actions.canDrop(draggedId, node.id);
  const isOver = nest.isOver && droppable;
  const showBefore = before.isOver && droppable;
  const showAfter = after.isOver && droppable;

  const rowRef = useMergedRefs<HTMLDivElement>(
    useCallback((el: HTMLDivElement | null) => registerRef(node.id, el), [
      registerRef,
      node.id,
    ]),
    drag.setNodeRef,
  );

  useEffect(() => {
    if (renaming) inputRef.current?.select();
  }, [renaming]);

  const commitRename = () => {
    const title = draft.trim() || "Untitled";
    if (title !== node.title) actions.rename(node.id, title);
    setRenaming(false);
  };

  const title = node.title || "Untitled";

  // One set of actions for the right-click menu and the "…" button.
  const renameChosen = useRef(false);
  const menu = [
    {
      key: "rename",
      label: "Rename",
      run: () => {
        renameChosen.current = true;
        setDraft(node.title);
        setRenaming(true);
      },
    },
    {
      key: "subpage",
      label: "Add subpage",
      run: () => actions.addSubpage(node.id),
    },
    {
      key: "favourite",
      label: node.is_favorite ? "Remove from favourites" : "Add to favourites",
      run: () => actions.setFavourite(node.id, !node.is_favorite),
    },
    {
      key: "delete",
      label: "Delete",
      destructive: true,
      run: () => setConfirmOpen(true),
    },
  ];
  // Returning focus to the button would pull it out of the rename field.
  const finalFocus = () => {
    const renamingNow = renameChosen.current;
    renameChosen.current = false;
    return !renamingNow;
  };
  // The "…" menu sits inside the row: its clicks and keys must not also open
  // or drag the page.
  const keepInMenu = {
    onClick: (e: React.SyntheticEvent) => e.stopPropagation(),
    onPointerDown: (e: React.SyntheticEvent) => e.stopPropagation(),
    onKeyDown: (e: React.SyntheticEvent) => e.stopPropagation(),
  };

  return (
    <>
      <div role="none" className="relative">
        <ContextMenu>
          <ContextMenuTrigger
            {...drag.attributes}
            {...drag.listeners}
            ref={rowRef}
            role="treeitem"
            aria-expanded={hasChildren ? expanded : undefined}
            aria-selected={isActive}
            tabIndex={-1}
            data-tree-id={node.id}
            onClick={() => openPage(node.id)}
            style={{
              paddingLeft: indentFor(depth),
              opacity: isDragging ? 0.4 : 1,
            }}
            className={`group flex h-7 cursor-pointer items-center gap-1.5 rounded-md pr-1 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring ${
              isOver ? "bg-brand-soft ring-2 ring-brand ring-inset" : ""
            } ${
              isActive
                ? "bg-brand-soft font-medium text-text"
                : "text-text-muted hover:bg-surface-hover hover:text-text"
            }`}
          >
            <button
              tabIndex={-1}
              aria-label={expanded ? "Collapse" : "Expand"}
              onClick={(e) => {
                e.stopPropagation();
                toggleExpanded(node.id);
              }}
              className={`grid size-4 shrink-0 place-items-center rounded text-text-faint hover:text-text ${
                hasChildren ? "" : "invisible"
              }`}
            >
              <ChevronRight
                className={`size-3 transition-transform ${expanded ? "rotate-90" : ""}`}
              />
            </button>

            <span className="grid size-4 shrink-0 place-items-center text-md">
              {node.icon ? (
                node.icon
              ) : node.type === "database" ? (
                <Database className="size-3.5 text-text-faint" />
              ) : (
                <FileText className="size-3.5 text-text-faint" />
              )}
            </span>

            {renaming ? (
              <input
                ref={inputRef}
                value={draft}
                aria-label="Page title"
                onChange={(e) => setDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                onBlur={commitRename}
                onKeyDown={(e) => {
                  // The tree's own key handling must not see these: space,
                  // arrows and Enter all belong to the field while it's open.
                  e.stopPropagation();
                  if (e.key === "Enter") commitRename();
                  else if (e.key === "Escape") {
                    setDraft(node.title);
                    setRenaming(false);
                  }
                }}
                className="min-w-0 flex-1 rounded border border-border bg-surface px-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              />
            ) : (
              <span className="min-w-0 flex-1 truncate" title={title}>
                {title}
              </span>
            )}

            <button
              tabIndex={-1}
              aria-label="Add subpage"
              onClick={(e) => {
                e.stopPropagation();
                actions.addSubpage(node.id);
              }}
              className="grid size-5 shrink-0 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-text group-hover:opacity-100 focus-visible:opacity-100"
            >
              <Plus className="size-3.5" />
            </button>
            <DropdownMenu>
              <DropdownMenuTrigger
                tabIndex={-1}
                aria-label={`Actions for ${title}`}
                {...keepInMenu}
                className="grid size-5 shrink-0 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-text group-hover:opacity-100 focus-visible:opacity-100 data-popup-open:opacity-100"
              >
                <MoreHorizontal className="size-3.5" />
              </DropdownMenuTrigger>
              <DropdownMenuContent
                align="end"
                className="w-44"
                finalFocus={finalFocus}
                {...keepInMenu}
              >
                {menu.map((item) => (
                  <Fragment key={item.key}>
                    {item.destructive && <DropdownMenuSeparator />}
                    <DropdownMenuItem
                      variant={item.destructive ? "destructive" : "default"}
                      onClick={item.run}
                    >
                      {item.label}
                    </DropdownMenuItem>
                  </Fragment>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </ContextMenuTrigger>

        {/* Base UI menu items activate through onClick; their onSelect is the
            DOM text-selection event and never fires here. */}
        <ContextMenuContent className="w-44">
            {menu.map((item) => (
              <Fragment key={item.key}>
                {item.destructive && <ContextMenuSeparator />}
                <ContextMenuItem
                  variant={item.destructive ? "destructive" : "default"}
                  onClick={item.run}
                >
                  {item.label}
                </ContextMenuItem>
              </Fragment>
            ))}
          </ContextMenuContent>
        </ContextMenu>

        {/* Drop zones: a thin strip top and bottom to reorder, the middle to
            nest. They only measure rects, so they never swallow a click. */}
        <span
          ref={before.setNodeRef}
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-0 h-1.5"
        />
        <span
          ref={nest.setNodeRef}
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-1.5 bottom-1.5"
        />
        <span
          ref={after.setNodeRef}
          aria-hidden
          className="pointer-events-none absolute inset-x-0 bottom-0 h-1.5"
        />

        {(showBefore || showAfter) && (
          <span
            aria-hidden
            style={{ left: indentFor(depth) }}
            className={`pointer-events-none absolute right-1 h-0.5 rounded-full bg-brand ${
              showBefore ? "top-0 -translate-y-px" : "bottom-0 translate-y-px"
            }`}
          />
        )}
      </div>

      {hasChildren && expanded && (
        <div role="group">
          {node.children.map((child) => (
            <PageTreeItem
              key={child.id}
              node={child}
              depth={depth + 1}
              registerRef={registerRef}
            />
          ))}
        </div>
      )}

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{title}"?</AlertDialogTitle>
            <AlertDialogDescription>
              {hasChildren
                ? "This page and all of its subpages will be permanently deleted."
                : "This page will be permanently deleted."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmOpen(false);
                actions.remove(node.id);
              }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
});
