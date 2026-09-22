import { Fragment, useEffect, useState } from "react";
import { ChevronDown } from "lucide-react";
import type { Field, FieldOptions } from "@/lib/api";
import {
  ChoicesEditor,
  FieldOptionsEditor,
  fieldOptionsValid,
  optionsFor,
} from "./AddFieldPopover";
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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

type Panel = "rename" | "options" | null;

const inputCls =
  "w-full rounded border border-border bg-surface px-2 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-brand/40";

const hasOptions = (t: Field["type"]) =>
  t === "relation" || t === "lookup" || t === "rollup" || t === "formula";

/**
 * A grid column header with its menu: rename, edit the field's choices or
 * options, and delete it (behind a confirmation). The same actions are on a
 * dropdown and on the header's right-click menu.
 */
export function FieldHeaderCell({
  field,
  fields,
  width,
  onRename,
  onUpdateOptions,
  onDelete,
  onResizeStart,
}: {
  field: Field;
  fields: Field[];
  width: number;
  onRename: (name: string) => void;
  onUpdateOptions: (options: FieldOptions) => void;
  onDelete: () => void;
  onResizeStart: (e: React.PointerEvent) => void;
}) {
  const [panel, setPanel] = useState<Panel>(null);
  const [confirm, setConfirm] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [name, setName] = useState(field.name);
  const [options, setOptions] = useState<FieldOptions>(field.options ?? {});

  // If this field is removed elsewhere, dismiss anything still open for it.
  const live = fields.some((f) => f.id === field.id);
  useEffect(() => {
    if (!live) {
      setPanel(null);
      setConfirm(false);
      setMenuOpen(false);
    }
  }, [live]);

  const openRename = () => {
    setName(field.name);
    setPanel("rename");
  };
  const openOptions = () => {
    setOptions(field.options ?? {});
    setPanel("options");
  };

  const actions: {
    key: string;
    label: string;
    run: () => void;
    destructive?: boolean;
  }[] = [
    { key: "rename", label: "Rename field", run: openRename },
    ...(field.type === "select"
      ? [{ key: "choices", label: "Edit choices", run: openOptions }]
      : []),
    ...(hasOptions(field.type)
      ? [{ key: "options", label: "Edit options", run: openOptions }]
      : []),
    { key: "delete", label: "Delete field", run: () => setConfirm(true), destructive: true },
  ];

  const optionsValid =
    field.type === "select" || fieldOptionsValid(field.type, options);

  const saveOptions = () => {
    const next = optionsFor(field.type, options);
    if (next) onUpdateOptions(next);
    setPanel(null);
  };

  const saveName = () => {
    const trimmed = name.trim();
    if (trimmed && trimmed !== field.name) onRename(trimmed);
    setPanel(null);
  };

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger
          style={{ width }}
          className="group/header relative flex items-center gap-1 border-r border-border px-3 py-2 text-xs font-semibold uppercase tracking-wide text-text-faint"
        >
          {/* truncation lives on the label so it can't clip the resize grip */}
          <span className="min-w-0 flex-1 truncate" title={field.name}>
            {field.name}
          </span>

          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger
              aria-label={`${field.name} column menu`}
              className="grid size-5 shrink-0 place-items-center rounded text-text-faint opacity-0 transition-opacity hover:bg-surface-hover hover:text-text focus-visible:opacity-100 group-hover/header:opacity-100 data-popup-open:opacity-100"
            >
              <ChevronDown className="size-3.5" />
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-44">
              {actions.map((a, i) => (
                <Fragment key={a.key}>
                  {a.destructive && i > 0 && <DropdownMenuSeparator />}
                  <DropdownMenuItem
                    variant={a.destructive ? "destructive" : "default"}
                    onClick={a.run}
                  >
                    {a.label}
                  </DropdownMenuItem>
                </Fragment>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>

          <span
            role="separator"
            aria-orientation="vertical"
            aria-label={`Resize ${field.name}`}
            onPointerDown={onResizeStart}
            style={{ touchAction: "none" }}
            className="group/grip absolute top-0 right-0 z-20 flex h-full w-2 translate-x-1/2 cursor-col-resize items-center justify-center"
          >
            <span className="h-2/3 w-0.5 rounded-full bg-transparent transition-colors group-hover/grip:bg-brand group-active/grip:bg-brand" />
          </span>
        </ContextMenuTrigger>

        <ContextMenuContent className="w-44">
          {actions.map((a, i) => (
            <Fragment key={a.key}>
              {a.destructive && i > 0 && <ContextMenuSeparator />}
              <ContextMenuItem
                variant={a.destructive ? "destructive" : "default"}
                onClick={a.run}
              >
                {a.label}
              </ContextMenuItem>
            </Fragment>
          ))}
        </ContextMenuContent>
      </ContextMenu>

      <Dialog
        open={panel !== null}
        onOpenChange={(o) => !o && setPanel(null)}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>
              {panel === "rename"
                ? "Rename field"
                : field.type === "select"
                  ? "Edit choices"
                  : "Edit options"}
            </DialogTitle>
            <DialogDescription>
              {panel === "rename"
                ? "Formulas reference fields by name, so update them too if you change this."
                : "Changes apply to every row in this database."}
            </DialogDescription>
          </DialogHeader>

          {panel === "rename" ? (
            <input
              autoFocus
              value={name}
              aria-label="Field name"
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveName();
              }}
              className={inputCls}
            />
          ) : panel === "options" ? (
            field.type === "select" ? (
              <ChoicesEditor
                choices={options.choices ?? []}
                onChange={(choices) => setOptions({ ...options, choices })}
              />
            ) : (
              <FieldOptionsEditor
                kind={field.type}
                fields={fields}
                options={options}
                onChange={setOptions}
              />
            )
          ) : null}

          <DialogFooter>
            <button
              onClick={() => setPanel(null)}
              className="rounded-md border border-border px-3 py-1.5 text-sm text-text-muted hover:bg-surface-hover"
            >
              Cancel
            </button>
            <button
              disabled={panel === "options" && !optionsValid}
              onClick={panel === "rename" ? saveName : saveOptions}
              className="rounded-md bg-brand px-3 py-1.5 text-sm font-medium text-white hover:bg-brand/90 disabled:opacity-40"
            >
              Save
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{field.name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This column and its values will be permanently deleted from every
              row.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                onDelete();
                setConfirm(false);
              }}
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
