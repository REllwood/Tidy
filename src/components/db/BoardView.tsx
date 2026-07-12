import {
  DndContext,
  PointerSensor,
  KeyboardSensor,
  useSensor,
  useSensors,
  useDraggable,
  useDroppable,
  type DragEndEvent,
} from "@dnd-kit/core";
import { Plus } from "lucide-react";
import type {
  DatabaseBundle,
  DbView,
  Field,
  RowWithCells,
  CellValue,
} from "@/lib/api";
import { useDbMutations } from "@/hooks/useDatabase";
import { CellDisplay } from "./cells";

const NONE = "__none__";

export function BoardView({
  bundle,
  view,
  pageId,
}: {
  bundle: DatabaseBundle;
  view: DbView;
  pageId: string;
}) {
  const m = useDbMutations(pageId);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor),
  );
  const { fields, rows, database_id } = bundle;
  const groupField = fields.find((f) => f.id === view.config?.groupByFieldId);
  const nameField = fields.find((f) => f.type === "text") ?? fields[0];

  if (!groupField || groupField.type !== "select") {
    return (
      <div className="p-8 text-sm text-text-faint">
        Group this database by a <b>Select</b> field to see a board.
      </div>
    );
  }

  const choices = groupField.options?.choices ?? [];
  const columns = [
    ...choices.map((c) => ({ id: c.id, name: c.name })),
    { id: NONE, name: "No status" },
  ];
  const rowsFor = (colId: string) =>
    rows.filter((r) =>
      colId === NONE ? r.cells[groupField.id] == null : r.cells[groupField.id] === colId,
    );

  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over) return;
    const rowId = String(e.active.id);
    const overId = String(e.over.id);

    // Dropped onto a card → place before it (and adopt that card's column).
    if (overId.startsWith("card:")) {
      const targetId = overId.slice(5);
      if (targetId === rowId) return;
      const target = rows.find((r) => r.id === targetId);
      if (!target) return;
      const colId = (target.cells[groupField.id] ?? NONE) as string;
      const colRows = rowsFor(colId).filter((r) => r.id !== rowId);
      const idx = colRows.findIndex((r) => r.id === targetId);
      const prev = colRows[idx - 1];
      const position = prev
        ? (prev.position + target.position) / 2
        : target.position - 1;
      const value: CellValue = colId === NONE ? null : colId;
      if ((rows.find((r) => r.id === rowId)?.cells[groupField.id] ?? NONE) !== colId)
        m.setCell.mutate({ rowId, fieldId: groupField.id, value });
      m.moveRow.mutate({ id: rowId, position });
      return;
    }

    // Dropped onto a column → set status + append to its end.
    const colId = overId.replace(/^col:/, "");
    const value: CellValue = colId === NONE ? null : colId;
    const colRows = rowsFor(colId).filter((r) => r.id !== rowId);
    const position =
      (colRows.length ? Math.max(...colRows.map((r) => r.position)) : 0) + 1;
    m.setCell.mutate({ rowId, fieldId: groupField.id, value });
    m.moveRow.mutate({ id: rowId, position });
  };

  const addCardTo = (colId: string) => {
    m.createRow.mutate(database_id, {
      onSuccess: (row) => {
        if (colId !== NONE)
          m.setCell.mutate({ rowId: row.id, fieldId: groupField.id, value: colId });
      },
    });
  };

  return (
    <DndContext sensors={sensors} onDragEnd={onDragEnd}>
      <div className="flex h-full items-start gap-4 overflow-x-auto p-5">
        {columns.map((col) => (
          <Column
            key={col.id}
            id={col.id}
            name={col.name}
            rows={rowsFor(col.id)}
            fields={fields}
            nameField={nameField}
            groupFieldId={groupField.id}
            onAdd={() => addCardTo(col.id)}
          />
        ))}
      </div>
    </DndContext>
  );
}

function Column({
  id,
  name,
  rows,
  fields,
  nameField,
  groupFieldId,
  onAdd,
}: {
  id: string;
  name: string;
  rows: RowWithCells[];
  fields: Field[];
  nameField: Field;
  groupFieldId: string;
  onAdd: () => void;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `col:${id}` });
  return (
    <section
      aria-label={name}
      className="flex w-72 shrink-0 flex-col gap-2 rounded-lg border border-border bg-bg-subtle p-2"
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
        ref={setNodeRef}
        className={`flex min-h-16 flex-col gap-2 rounded-md p-0.5 ${
          isOver ? "ring-2 ring-brand ring-inset" : ""
        }`}
      >
        {rows.map((r) => (
          <Card
            key={r.id}
            row={r}
            nameField={nameField}
            secondary={fields.filter(
              (f) => f.id !== nameField.id && f.id !== groupFieldId,
            )}
          />
        ))}
      </div>
    </section>
  );
}

function Card({
  row,
  nameField,
  secondary,
}: {
  row: RowWithCells;
  nameField: Field;
  secondary: Field[];
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
      {...drag.attributes}
      {...drag.listeners}
      style={{ opacity: drag.isDragging ? 0.4 : 1 }}
      className={`cursor-grab rounded-md border bg-surface p-2.5 shadow-sm hover:border-border-strong ${
        drop.isOver && !drag.isDragging ? "border-brand" : "border-border"
      }`}
    >
      <div className="text-sm font-medium">
        {row.cells[nameField.id] || "Untitled"}
      </div>
      {secondary.length > 0 && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          {secondary.map((f) =>
            row.cells[f.id] != null && row.cells[f.id] !== "" ? (
              <CellDisplay key={f.id} field={f} value={row.cells[f.id]} />
            ) : null,
          )}
        </div>
      )}
    </div>
  );
}
