import { useRef, useState } from "react";
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
} from "@/lib/api";
import { databasesApi } from "@/lib/api";
import { useDbMutations } from "@/hooks/useDatabase";
import { useUi } from "@/store/ui";
import { isComputed, computeCell } from "@/lib/computed";
import { EditableCell } from "./EditableCell";
import { RelationCell } from "./RelationCell";
import { ComputedCell } from "./cells";
import { AddFieldPopover } from "./AddFieldPopover";

const COL_W = 200;
const MIN_W = 80;

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

  const persisted = view?.config?.columnWidths ?? {};
  const [widths, setWidths] = useState<Record<string, number>>(persisted);
  const widthOf = (id: string) => widths[id] ?? persisted[id] ?? COL_W;

  const startResize = (fieldId: string, e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = widthOf(fieldId);
    const onMove = (ev: PointerEvent) => {
      const w = Math.max(MIN_W, startW + (ev.clientX - startX));
      setWidths((prev) => ({ ...prev, [fieldId]: w }));
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      // persist the final widths to the view config
      setWidths((final) => {
        if (view && onUpdate)
          onUpdate({ ...view.config, columnWidths: final });
        return final;
      });
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 38,
    overscan: 12,
  });

  const totalW = fields.reduce((sum, f) => sum + widthOf(f.id), 0) + 68;
  const gridTemplate = `${fields.map((f) => `${widthOf(f.id)}px`).join(" ")} 68px`;

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
              <div
                key={f.id}
                className="relative truncate border-r border-border px-3 py-2 text-xs font-semibold uppercase tracking-wide text-text-faint"
              >
                {f.name}
                <span
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={`Resize ${f.name}`}
                  onPointerDown={(e) => startResize(f.id, e)}
                  className="absolute -right-1 top-0 z-20 h-full w-2 cursor-col-resize hover:bg-brand/40"
                />
              </div>
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
            <div className="flex flex-col items-center px-6 py-14 text-center">
              <span className="mb-2 grid size-10 place-items-center rounded-full bg-brand-soft text-brand">
                <Rows3 className="size-5" />
              </span>
              <div className="text-sm font-semibold text-text">No rows yet</div>
              <p className="mt-1 text-note text-text-muted">
                Add your first row below to get started.
              </p>
            </div>
          ) : (
            <div
              style={{ height: rowVirtualizer.getTotalSize(), position: "relative" }}
            >
              {rowVirtualizer.getVirtualItems().map((vi) => {
                const row = rows[vi.index];
                return (
                  <div
                    key={row.id}
                    className="group absolute left-0 grid w-full border-b border-border/50 transition-colors hover:bg-surface-hover/40"
                    style={{
                      gridTemplateColumns: gridTemplate,
                      transform: `translateY(${vi.start}px)`,
                      height: 38,
                    }}
                  >
                    {fields.map((f) => (
                      <div
                        key={f.id}
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
                            target={targetBundles[f.options?.targetDatabaseId ?? ""]}
                            onChange={(value: CellValue) =>
                              m.setCell.mutate({ rowId: row.id, fieldId: f.id, value })
                            }
                          />
                        ) : (
                          <EditableCell
                            field={f}
                            value={row.cells[f.id]}
                            onChange={(value: CellValue) =>
                              m.setCell.mutate({ rowId: row.id, fieldId: f.id, value })
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
                        className="grid size-6 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-danger-c group-hover:opacity-100"
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
        onClick={() => m.createRow.mutate(database_id)}
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
  const [busy, setBusy] = useState(false);
  const title = String(row.cells[fields[0]?.id] ?? "").trim() || "Untitled";

  const promote = async () => {
    setBusy(true);
    try {
      const page = await databasesApi.promoteRow(row.id, title);
      toast.success(`Opened "${title}" as a page`);
      openPage(page.id);
    } catch {
      toast.error("Couldn't open row as page");
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
      className="grid size-6 place-items-center rounded text-text-faint opacity-0 transition-opacity duration-150 hover:bg-surface hover:text-brand group-hover:opacity-100 disabled:opacity-40"
    >
      <FileSymlink className="size-3.5" />
    </button>
  );
}

