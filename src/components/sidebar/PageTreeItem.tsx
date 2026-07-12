import { useEffect, useRef, useState } from "react";
import { useDraggable, useDroppable } from "@dnd-kit/core";
import { ChevronRight, FileText, Database, Plus, MoreHorizontal } from "lucide-react";
import type { PageNode } from "@/lib/api";
import { mergeRefs } from "@/lib/utils";
import { useUi } from "@/store/ui";
import {
  useRenamePage,
  useSetFavorite,
  useDeletePage,
  useCreatePage,
} from "@/hooks/usePages";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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

export function PageTreeItem({
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
  const setExpanded = useUi((s) => s.setExpanded);
  const openPage = useUi((s) => s.openPage);
  const activePane = useUi((s) => s.activePane);
  const isActive = activePane.kind === "page" && activePane.pageId === node.id;

  const rename = useRenamePage();
  const setFavorite = useSetFavorite();
  const del = useDeletePage();
  const create = useCreatePage();

  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(node.title);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const hasChildren = node.children.length > 0;

  const drag = useDraggable({ id: node.id });
  const drop = useDroppable({ id: node.id });
  const isDragging = drag.isDragging;
  const isOver = drop.isOver && !isDragging;

  useEffect(() => {
    if (renaming) inputRef.current?.select();
  }, [renaming]);

  const commitRename = () => {
    const title = draft.trim() || "Untitled";
    if (title !== node.title) rename.mutate({ id: node.id, title });
    setRenaming(false);
  };

  const addSubpage = async () => {
    setExpanded(node.id, true);
    create.mutate({ parentId: node.id, title: "Untitled" });
  };

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger
          {...drag.attributes}
          {...drag.listeners}
          ref={mergeRefs<HTMLDivElement>(
            (el) => registerRef(node.id, el),
            drag.setNodeRef,
            drop.setNodeRef,
          )}
          role="treeitem"
          aria-expanded={hasChildren ? expanded : undefined}
          aria-selected={isActive}
          tabIndex={-1}
          data-tree-id={node.id}
          onClick={() => openPage(node.id)}
          style={{ paddingLeft: 8 + depth * 14, opacity: isDragging ? 0.4 : 1 }}
          className={`group flex h-7 cursor-pointer items-center gap-1.5 rounded-md pr-1 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring ${
            isOver ? "ring-2 ring-brand ring-inset bg-brand-soft" : ""
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
                onChange={(e) => setDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                onBlur={commitRename}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename();
                  else if (e.key === "Escape") {
                    setDraft(node.title);
                    setRenaming(false);
                  }
                }}
                className="min-w-0 flex-1 rounded border border-border bg-surface px-1 text-sm outline-none"
              />
            ) : (
              <span className="min-w-0 flex-1 truncate">{node.title || "Untitled"}</span>
            )}

            <button
              tabIndex={-1}
              aria-label="Add subpage"
              onClick={(e) => {
                e.stopPropagation();
                addSubpage();
              }}
              className="grid size-5 shrink-0 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-text group-hover:opacity-100"
            >
              <Plus className="size-3.5" />
            </button>
            <span
              aria-hidden
              className="grid size-5 shrink-0 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 group-hover:opacity-100"
            >
              <MoreHorizontal className="size-3.5" />
            </span>
        </ContextMenuTrigger>

        <ContextMenuContent className="w-44">
          <ContextMenuItem
            onSelect={() => {
              setDraft(node.title);
              setRenaming(true);
            }}
          >
            Rename
          </ContextMenuItem>
          <ContextMenuItem onSelect={addSubpage}>Add subpage</ContextMenuItem>
          <ContextMenuItem
            onSelect={() =>
              setFavorite.mutate({ id: node.id, isFavorite: !node.is_favorite })
            }
          >
            {node.is_favorite ? "Remove from favourites" : "Add to favourites"}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem
            variant="destructive"
            onSelect={() => setConfirmOpen(true)}
          >
            Delete
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>

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
            <AlertDialogTitle>Delete "{node.title || "Untitled"}"?</AlertDialogTitle>
            <AlertDialogDescription>
              {hasChildren
                ? "This page and all of its subpages will be permanently deleted."
                : "This page will be permanently deleted."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => del.mutate(node.id)}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
