import { useCallback, useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Plus, Trash2, FileSymlink, Rows3 } from "lucide-react";
import { toast } from "sonner";
import type {
  DatabaseBundle,
  DbView,
  CellValue,
  ViewConfig,
  RowWithCells,
  Field,
  FieldOptions,
} from "@/lib/api";
import { databasesApi } from "@/lib/api";
import { useQueryClient } from "@tanstack/react-query";
import { useDbMutations } from "@/hooks/useDatabase";
import { rememberPage } from "@/hooks/usePages";
import { useUi } from "@/store/ui";
import { isComputed, computeCell } from "@/lib/computed";
import { errorMessage } from "@/lib/errors";
import { EditableCell } from "./EditableCell";
import { RelationCell } from "./RelationCell";
import { ComputedCell } from "./cells";
import { AddFieldPopover } from "./AddFieldPopover";
import { FieldHeaderCell } from "./FieldMenu";
import { DbEmptyState } from "./EmptyState";

const COL_W = 200;
const MIN_W = 80;
const ROW_H = 38;
const ACTIONS_W = 68;

export function GridView({
  bundle,
  pageId,
  view,
  onUpdate,
  targetBundles = {},
}: {
  bundle: DatabaseBundle;
  pageId: string;
  view?: DbView;
  onUpdate?: (config: ViewConfig) => void;
  targetBundles?: Record<string, DatabaseBundle>;
}) {
  const m = useDbMutations(pageId);
  const parentRef = useRef<HTMLDivElement>(null);
  const { fields, rows, database_id } = bundle;

  const persisted = view?.config?.columnWidths;
  const [widths, setWidths] = useState<Record<string, number>>(
    () => persisted ?? {},
  );
  // Mirror the live widths so the pointer handlers never read stale state.
  const widthsRef = useRef(widths);
  widthsRef.current = widths;

  // Adopt widths written elsewhere (another window, or "Reset column widths").
  const lastPersisted = useRef(persisted);
  useEffect(() => {
    if (persisted !== lastPersisted.current) {
      lastPersisted.current = persisted;
      setWidths(persisted ?? {});
    }
  }, [persisted]);

  const widthOf = (id: string) => widths[id] ?? COL_W;

  const startResize = (fieldId: string, e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = widthOf(fieldId);
    let latest: Record<string, number> | null = null;
    let settled = false;

    const onMove = (ev: PointerEvent) => {
      const w = Math.max(MIN_W, Math.round(startW + (ev.clientX - startX)));
      latest = { ...widthsRef.current, [fieldId]: w };
      setWidths(latest);
    };
    const onUp = () => {
      // Persist exactly once, outside any state updater: a setState callback
      // runs twice under StrictMode and would fire two writes.
      if (settled) return;
      settled = true;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      if (latest && view && onUpdate)
        onUpdate({ ...(view.config ?? {}), columnWidths: latest });
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_H,
    overscan: 12,
  });

  // Focus the first editable cell of a row we just added.
  const [focusRowId, setFocusRowId] = useState<string | null>(null);
  const scrollToIndex = rowVirtualizer.scrollToIndex;
  useEffect(() => {
    if (!focusRowId) return;
    const index = rows.findIndex((r) => r.id === focusRowId);
    if (index < 0) return;
    scrollToIndex(index, { align: "end" });
    const frame = requestAnimationFrame(() => {
      const control = parentRef.current?.querySelector<HTMLElement>(
        `[data-row="${focusRowId}"] [data-cell] input, [data-row="${focusRowId}"] [data-cell] button`,
      );
      control?.focus();
      setFocusRowId(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [focusRowId, rows, scrollToIndex]);

  const addRow = useCallback(() => {
    m.createRow.mutate(database_id, {
      onSuccess: (row) => setFocusRowId(row.id),
    });
  }, [m.createRow, database_id]);

  const totalW =
    fields.reduce((sum, f) => sum + widthOf(f.id), 0) + ACTIONS_W;
  const gridTemplate = `${fields
    .map((f) => `${widthOf(f.id)}px`)
    .join(" ")} ${ACTIONS_W}px`;

  return (
    <div className="flex h-full flex-col">
      <div ref={parentRef} className="flex-1 overflow-auto">
        <div style={{ minWidth: totalW }}>
          {/* header */}
          <div
            className="sticky top-0 z-10 grid border-b border-border bg-bg"
            style={{ gridTemplateColumns: gridTemplate }}
          >
            {fields.map((f) => (
              <FieldHeaderCell
                key={f.id}
                field={f}
                fields={fields}
                width={widthOf(f.id)}
                onRename={(name) => m.updateField.mutate({ id: f.id, name })}
                onUpdateOptions={(options: FieldOptions) =>
                  m.updateField.mutate({ id: f.id, options })
                }
                onDelete={() => m.deleteField.mutate(f.id)}
                onResizeStart={(e) => startResize(f.id, e)}
              />
            ))}
            <div className="grid place-items-center">
              <AddFieldPopover
                databaseId={database_id}
                fields={fields}
                onAdd={m.createField.mutate}
              />
            </div>
          </div>

          {/* virtualized rows */}
          {rows.length === 0 ? (
            <div className="p-6">
              <DbEmptyState
                icon={<Rows3 className="size-5" />}
                title={fields.length ? "No rows yet" : "This database is empty"}
                hint={
                  fields.length
                    ? "Add your first row below to get started."
                    : "Add a column with the + above, then add a row."
                }
                action={
                  <button
                    onClick={addRow}
                    className="flex items-center gap-1.5 rounded-md bg-brand px-3 py-1.5 text-sm font-medium text-white hover:bg-brand/90"
                  >
                    <Plus className="size-4" /> New row
                  </button>
                }
              />
            </div>
          ) : (
            <div
              style={{
                height: rowVirtualizer.getTotalSize(),
                position: "relative",
              }}
            >
              {rowVirtualizer.getVirtualItems().map((vi) => {
                const row = rows[vi.index];
                return (
                  <div
                    key={row.id}
                    data-row={row.id}
                    className="group absolute left-0 grid w-full border-b border-border/50 transition-colors hover:bg-surface-hover/40"
                    style={{
                      gridTemplateColumns: gridTemplate,
                      transform: `translateY(${vi.start}px)`,
                      height: ROW_H,
                    }}
                  >
                    {fields.map((f) => (
                      <div
                        key={f.id}
                        data-cell={f.id}
                        className="flex items-center overflow-hidden border-r border-border px-3"
                      >
                        {isComputed(f.type) ? (
                          <ComputedCell
                            field={f}
                            value={computeCell(f, row, fields, targetBundles)}
                          />
                        ) : f.type === "relation" ? (
                          <RelationCell
                            field={f}
                            value={row.cells[f.id]}
                            target={
                              targetBundles[f.options?.targetDatabaseId ?? ""]
                            }
                            onChange={(value: CellValue) =>
                              m.setCell.mutate({
                                rowId: row.id,
                                fieldId: f.id,
                                value,
                              })
                            }
                          />
                        ) : (
                          <EditableCell
                            field={f}
                            value={row.cells[f.id]}
                            onChange={(value: CellValue) =>
                              m.setCell.mutate({
                                rowId: row.id,
                                fieldId: f.id,
                                value,
                              })
                            }
                          />
                        )}
                      </div>
                    ))}
                    <div className="flex items-center justify-center gap-0.5">
                      <PromoteButton row={row} fields={fields} />
                      <button
                        aria-label="Delete row"
                        onClick={() => m.deleteRow.mutate(row.id)}
                        className="grid size-6 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-danger-c focus-visible:opacity-100 group-hover:opacity-100"
                      >
                        <Trash2 className="size-3.5" />
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      <button
        onClick={addRow}
        className="flex items-center gap-2 border-t border-border px-3 py-2 text-sm text-text-muted hover:bg-surface-hover hover:text-text"
      >
        <Plus className="size-4" /> New row
      </button>
    </div>
  );
}

/** Promote a row into a full editable page ("record"), then open it. */
function PromoteButton({
  row,
  fields,
}: {
  row: RowWithCells;
  fields: Field[];
}) {
  const openPage = useUi((s) => s.openPage);
  const qc = useQueryClient();
  const [busy, setBusy] = useState(false);
  const title = String(row.cells[fields[0]?.id] ?? "").trim() || "Untitled";

  const promote = async () => {
    setBusy(true);
    try {
      const page = await databasesApi.promoteRow(row.id, title);
      rememberPage(qc, page);
      toast.success(`Opened "${title}" as a page`);
      openPage(page.id);
    } catch (error) {
      toast.error("Couldn't open this row as a page", {
        description: errorMessage(error),
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      aria-label="Open row as page"
      title="Open as page"
      disabled={busy}
      onClick={promote}
      className="grid size-6 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-brand focus-visible:opacity-100 group-hover:opacity-100 disabled:opacity-40"
    >
      <FileSymlink className="size-3.5" />
    </button>
  );
}
