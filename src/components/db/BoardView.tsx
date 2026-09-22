import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  KeyboardSensor,
  useSensor,
  useSensors,
  useDraggable,
  useDroppable,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Columns3, GripVertical, Plus } from "lucide-react";
import type {
  DatabaseBundle,
  DbView,
  Field,
  RowWithCells,
  CellValue,
} from "@/lib/api";
import { useDbMutations } from "@/hooks/useDatabase";
import { isComputed, computeCell } from "@/lib/computed";
import { CellDisplay } from "./cells";
import { EditableCell } from "./EditableCell";
import { DbEmptyState, MissingFieldNotice } from "./EmptyState";
import { boardGroupField, linkText, titleField } from "./viewFields";
import {
  NONE,
  columnOf,
  planDropAtEnd,
  planDropBefore,
  type DropPlan,
} from "./boardOrder";

const CARD_EST = 68;

export function BoardView({
  bundle,
  view,
  pageId,
  allRows,
  targetBundles = {},
}: {
  bundle: DatabaseBundle;
  view: DbView;
  pageId: string;
  /** The unfiltered row set: drop positions must account for hidden rows. */
  allRows: RowWithCells[];
  targetBundles?: Record<string, DatabaseBundle>;
}) {
  const m = useDbMutations(pageId);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor),
  );
  const { fields, rows, database_id } = bundle;
  const group = boardGroupField(fields, view.config);
  const groupField = group.field;
  const nameField = titleField(fields);
  const [dragging, setDragging] = useState<string | null>(null);
  const [focusRowId, setFocusRowId] = useState<string | null>(null);

  const rowsFor = useCallback(
    (colId: string) =>
      groupField
        ? rows
            .filter((r) => columnOf(r, groupField.id) === colId)
            .sort((a, b) => a.position - b.position)
        : [],
    [rows, groupField],
  );

  const setCell = useCallback(
    (rowId: string, fieldId: string, value: CellValue) =>
      m.setCell.mutate({ rowId, fieldId, value }),
    [m.setCell],
  );

  const applyPlan = useCallback(
    (rowId: string, plan: DropPlan) => {
      // Renumber the neighbours first when the fractional gap has collapsed.
      for (const p of plan.renormalise ?? []) {
        if (p.id === rowId) continue;
        m.moveRow.mutate(p);
      }
      m.moveRow.mutate({ id: rowId, position: plan.position });
    },
    [m.moveRow],
  );

  const onDragEnd = (e: DragEndEvent) => {
    setDragging(null);
    if (!e.over || !groupField) return;
    const rowId = String(e.active.id);
    const overId = String(e.over.id);

    // Dropped onto a card → place before it (and adopt that card's column).
    if (overId.startsWith("card:")) {
      const targetId = overId.slice(5);
      if (targetId === rowId) return;
      const target = allRows.find((r) => r.id === targetId);
      if (!target) return;
      const colId = columnOf(target, groupField.id);
      // Always write the column value: the card may have been dragged across
      // columns, and a no-op write is cheaper than a wrong column.
      m.setCell.mutate({
        rowId,
        fieldId: groupField.id,
        value: (colId === NONE ? null : colId) as CellValue,
      });
      const plan = planDropBefore(allRows, groupField.id, rowId, targetId);
      if (plan) applyPlan(rowId, plan);
      return;
    }

    // Dropped onto a column → set its value and append to the end.
    const colId = overId.replace(/^col:/, "");
    m.setCell.mutate({
      rowId,
      fieldId: groupField.id,
      value: (colId === NONE ? null : colId) as CellValue,
    });
    applyPlan(rowId, planDropAtEnd(allRows));
  };

  const addCardTo = (colId: string) => {
    m.createRow.mutate(database_id, {
      onSuccess: (row) => {
        if (colId !== NONE && groupField)
          m.setCell.mutate({
            rowId: row.id,
            fieldId: groupField.id,
            value: colId,
          });
        setFocusRowId(row.id);
      },
    });
  };

  // Focus the new card's first editable cell once it has rendered.
  useEffect(() => {
    if (!focusRowId) return;
    if (!rows.some((r) => r.id === focusRowId)) return;
    const frame = requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(`[data-row="${focusRowId}"] [data-cell] input`)
        ?.focus();
      setFocusRowId(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [focusRowId, rows]);

  const secondary = useMemo(
    () =>
      fields.filter(
        (f) => f.id !== nameField?.id && f.id !== groupField?.id,
      ),
    [fields, nameField?.id, groupField?.id],
  );

  const draggedRow = dragging
    ? rows.find((r) => r.id === dragging) ?? null
    : null;

  if (!groupField || !nameField) {
    return (
      <div className="p-6">
        {group.missing && <MissingFieldNotice className="mb-4" />}
        <DbEmptyState
          icon={<Columns3 className="size-5" />}
          title="No board columns yet"
          hint="A board groups rows by a Select field. Add one, then choose it in View settings."
        />
      </div>
    );
  }

  const choices = groupField.options?.choices ?? [];
  const columns = [
    ...choices.map((c) => ({ id: c.id, name: c.name })),
    { id: NONE, name: "No status" },
  ];

  return (
    <DndContext
      sensors={sensors}
      onDragStart={(e: DragStartEvent) => setDragging(String(e.active.id))}
      onDragCancel={() => setDragging(null)}
      onDragEnd={onDragEnd}
    >
      <div className="flex h-full flex-col">
        {group.missing && (
          <div className="px-5 pt-4">
            <MissingFieldNotice />
          </div>
        )}
        {rows.length === 0 && (
          <div className="px-5 pt-4">
            <DbEmptyState
              icon={<Columns3 className="size-5" />}
              title="Nothing on the board"
              hint="Add a card to a column below, or clear the filters to see more."
            />
          </div>
        )}
        <div className="flex min-h-0 flex-1 items-stretch gap-4 overflow-x-auto p-5">
          {columns.map((col) => (
            <Column
              key={col.id}
              id={col.id}
              name={col.name}
              rows={rowsFor(col.id)}
              nameField={nameField}
              secondary={secondary}
              fields={fields}
              targetBundles={targetBundles}
              onSetCell={setCell}
              onAdd={() => addCardTo(col.id)}
            />
          ))}
        </div>
      </div>

      <DragOverlay dropAnimation={null}>
        {draggedRow ? (
          <div className="w-64 rounded-md border border-brand bg-surface p-2.5 text-sm font-medium shadow-md">
            {String(draggedRow.cells[nameField.id] ?? "") || "Untitled"}
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

function Column({
  id,
  name,
  rows,
  nameField,
  secondary,
  fields,
  targetBundles,
  onSetCell,
  onAdd,
}: {
  id: string;
  name: string;
  rows: RowWithCells[];
  nameField: Field;
  secondary: Field[];
  fields: Field[];
  targetBundles: Record<string, DatabaseBundle>;
  onSetCell: (rowId: string, fieldId: string, value: CellValue) => void;
  onAdd: () => void;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `col:${id}` });
  const bodyRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => bodyRef.current,
    estimateSize: () => CARD_EST,
    overscan: 8,
  });

  const setRefs = (el: HTMLDivElement | null) => {
    bodyRef.current = el;
    setNodeRef(el);
  };

  return (
    <section
      aria-label={name}
      className="flex h-full w-72 shrink-0 flex-col gap-2 rounded-lg border border-border bg-bg-subtle p-2"
    >
      <div className="flex items-center gap-2 px-1.5 pt-1">
        <span className="text-note font-semibold">{name}</span>
        <span className="text-xs text-text-faint">{rows.length}</span>
        <button
          aria-label={`Add card to ${name}`}
          onClick={onAdd}
          className="ml-auto grid size-6 place-items-center rounded text-text-faint hover:bg-surface-hover hover:text-text"
        >
          <Plus className="size-4" />
        </button>
      </div>
      <div
        ref={setRefs}
        className={`min-h-16 flex-1 overflow-y-auto rounded-md p-0.5 ${
          isOver ? "ring-2 ring-brand ring-inset" : ""
        }`}
      >
        {rows.length === 0 ? (
          <p className="px-2 py-4 text-center text-note text-text-faint">
            Drop a card here
          </p>
        ) : (
          <div
            style={{ height: virtualizer.getTotalSize(), position: "relative" }}
          >
            {virtualizer.getVirtualItems().map((vi) => (
              <div
                key={rows[vi.index].id}
                data-index={vi.index}
                ref={virtualizer.measureElement}
                className="absolute left-0 w-full pb-2"
                style={{ transform: `translateY(${vi.start}px)` }}
              >
                <Card
                  row={rows[vi.index]}
                  nameField={nameField}
                  secondary={secondary}
                  fields={fields}
                  targetBundles={targetBundles}
                  onSetCell={onSetCell}
                />
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function Card({
  row,
  nameField,
  secondary,
  fields,
  targetBundles,
  onSetCell,
}: {
  row: RowWithCells;
  nameField: Field;
  secondary: Field[];
  fields: Field[];
  targetBundles: Record<string, DatabaseBundle>;
  onSetCell: (rowId: string, fieldId: string, value: CellValue) => void;
}) {
  const drag = useDraggable({ id: row.id });
  const drop = useDroppable({ id: `card:${row.id}` });
  const setRef = (el: HTMLElement | null) => {
    drag.setNodeRef(el);
    drop.setNodeRef(el);
  };

  return (
    <div
      ref={setRef}
      data-row={row.id}
      style={{ opacity: drag.isDragging ? 0.4 : 1 }}
      className={`rounded-md border bg-surface p-2.5 shadow-sm transition-colors hover:border-border-strong ${
        drop.isOver && !drag.isDragging ? "border-brand" : "border-border"
      }`}
    >
      <div className="flex items-start gap-1.5">
        <button
          {...drag.attributes}
          {...drag.listeners}
          aria-label={`Move ${String(row.cells[nameField.id] ?? "") || "Untitled"}`}
          className="mt-0.5 cursor-grab touch-none rounded text-text-faint hover:text-text focus-visible:ring-2 focus-visible:ring-brand/50"
        >
          <GripVertical className="size-3.5" />
        </button>
        <div data-cell={nameField.id} className="min-w-0 flex-1 text-sm font-medium">
          <EditableCell
            field={nameField}
            value={row.cells[nameField.id]}
            onChange={(value: CellValue) =>
              onSetCell(row.id, nameField.id, value)
            }
          />
        </div>
      </div>
      {secondary.length > 0 && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5 pl-5">
          {secondary.map((f) => {
            const value =
              f.type === "relation" || f.type === "dependencies"
                ? linkText(row, f, targetBundles)
                : isComputed(f.type)
                  ? computeCell(f, row, fields, targetBundles)
                  : row.cells[f.id];
            if (value == null || value === "") return null;
            return (
              <span key={f.id} title={f.name} className="max-w-full truncate">
                <CellDisplay field={f} value={value} />
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}
