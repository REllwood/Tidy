import { useEffect, useRef, useState } from "react";
import { ListFilter, ArrowUpDown, Plus, Settings2, X } from "lucide-react";
import type {
  DatabaseBundle,
  CellValue,
  DbView,
  Field,
  ViewConfig,
} from "@/lib/api";
import {
  defaultOp,
  isValueless,
  opsForType,
  OP_LABELS,
  type Filter,
  type FilterOp,
  type Sort,
} from "@/lib/dbFilters";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

const COMMIT_DELAY = 300;

const controlCls =
  "min-w-0 rounded border border-border bg-surface px-1.5 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-brand/40";

const triggerCls =
  "flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-note transition-colors hover:bg-surface-hover";

export function DbToolbar({
  bundle,
  view,
  onUpdate,
}: {
  bundle: DatabaseBundle;
  view: DbView;
  onUpdate: (config: ViewConfig) => void;
}) {
  const cfg = view.config ?? {};
  const filters = (cfg.filters ?? []) as Filter[];
  const sorts = (cfg.sorts ?? []) as Sort[];
  const fields = bundle.fields;

  const setFilters = (next: Filter[]) => onUpdate({ ...cfg, filters: next });
  const setSorts = (next: Sort[]) => onUpdate({ ...cfg, sorts: next });

  return (
    <div className="flex items-center gap-1.5">
      {/* Filter */}
      <Popover>
        <PopoverTrigger
          className={`${triggerCls} ${filters.length ? "text-brand" : "text-text-muted"}`}
        >
          <ListFilter className="size-3.5" />
          Filter{filters.length ? ` (${filters.length})` : ""}
        </PopoverTrigger>
        <PopoverContent className="w-96 p-2">
          {filters.length === 0 && (
            <p className="px-1 py-1 text-note text-text-faint">No filters.</p>
          )}
          <div className="space-y-1.5">
            {filters.map((f, i) => (
              <FilterRow
                key={`${f.fieldId}-${i}`}
                filter={f}
                field={fields.find((x) => x.id === f.fieldId)}
                onChange={(nf) =>
                  setFilters(filters.map((x, j) => (j === i ? nf : x)))
                }
                onRemove={() => setFilters(filters.filter((_, j) => j !== i))}
              />
            ))}
          </div>
          <AddControl
            fields={fields}
            label="Add filter"
            onPick={(field) =>
              setFilters([
                ...filters,
                { fieldId: field.id, op: defaultOp(field.type), value: "" },
              ])
            }
          />
        </PopoverContent>
      </Popover>

      {/* Sort */}
      <Popover>
        <PopoverTrigger
          className={`${triggerCls} ${sorts.length ? "text-brand" : "text-text-muted"}`}
        >
          <ArrowUpDown className="size-3.5" />
          Sort{sorts.length ? ` (${sorts.length})` : ""}
        </PopoverTrigger>
        <PopoverContent className="w-72 p-2">
          {sorts.length === 0 && (
            <p className="px-1 py-1 text-note text-text-faint">No sorts.</p>
          )}
          <div className="space-y-1.5">
            {sorts.map((s, i) => {
              const field = fields.find((f) => f.id === s.fieldId);
              return (
                <div key={`${s.fieldId}-${i}`} className="flex items-center gap-1.5">
                  <span
                    className={`flex-1 truncate text-sm ${field ? "" : "text-text-faint italic"}`}
                  >
                    {field?.name ?? "Deleted field"}
                  </span>
                  <button
                    onClick={() =>
                      setSorts(
                        sorts.map((x, j) =>
                          j === i
                            ? { ...x, dir: x.dir === "asc" ? "desc" : "asc" }
                            : x,
                        ),
                      )
                    }
                    className="rounded border border-border px-2 py-0.5 text-xs text-text-muted hover:bg-surface-hover"
                  >
                    {s.dir === "asc" ? "Asc ↑" : "Desc ↓"}
                  </button>
                  <button
                    aria-label="Remove sort"
                    onClick={() => setSorts(sorts.filter((_, j) => j !== i))}
                    className="grid size-6 place-items-center rounded text-text-faint hover:text-danger-c"
                  >
                    <X className="size-3.5" />
                  </button>
                </div>
              );
            })}
          </div>
          <AddControl
            fields={fields}
            label="Add sort"
            onPick={(field) =>
              setSorts([...sorts, { fieldId: field.id, dir: "asc" }])
            }
          />
        </PopoverContent>
      </Popover>

      {/* View settings */}
      <Popover>
        <PopoverTrigger
          aria-label="View settings"
          className={`${triggerCls} text-text-muted`}
        >
          <Settings2 className="size-3.5" />
          View
        </PopoverTrigger>
        <PopoverContent className="w-72 p-2">
          <ViewSettings
            kind={view.kind}
            cfg={cfg}
            fields={fields}
            onUpdate={onUpdate}
          />
        </PopoverContent>
      </Popover>
    </div>
  );
}

function ViewSettings({
  kind,
  cfg,
  fields,
  onUpdate,
}: {
  kind: DbView["kind"];
  cfg: ViewConfig;
  fields: Field[];
  onUpdate: (config: ViewConfig) => void;
}) {
  const of = (type: Field["type"]) => fields.filter((f) => f.type === type);
  const set = (patch: Partial<ViewConfig>) => onUpdate({ ...cfg, ...patch });

  return (
    <div className="space-y-2">
      {kind === "board" && (
        <FieldSelect
          label="Group by"
          hint="Board columns come from a Select field."
          value={cfg.groupByFieldId}
          options={of("select")}
          onChange={(groupByFieldId) => set({ groupByFieldId })}
        />
      )}
      {kind === "calendar" && (
        <FieldSelect
          label="Date field"
          hint="Rows appear on the day in this field."
          value={cfg.dateFieldId}
          options={of("date")}
          onChange={(dateFieldId) => set({ dateFieldId })}
        />
      )}
      {kind === "gantt" && (
        <>
          <FieldSelect
            label="Start date"
            value={cfg.startFieldId}
            options={of("date")}
            onChange={(startFieldId) => set({ startFieldId })}
          />
          <FieldSelect
            label="End date"
            value={cfg.endFieldId}
            options={of("date")}
            onChange={(endFieldId) => set({ endFieldId })}
          />
          <FieldSelect
            label="Dependencies"
            value={cfg.dependenciesFieldId}
            options={of("dependencies")}
            onChange={(dependenciesFieldId) => set({ dependenciesFieldId })}
          />
        </>
      )}
      {kind === "grid" && (
        <>
          <p className="px-0.5 text-note text-text-muted">
            Columns you have resized are remembered for this view.
          </p>
          <button
            onClick={() => set({ columnWidths: {} })}
            disabled={!Object.keys(cfg.columnWidths ?? {}).length}
            className="w-full rounded-md border border-border px-2 py-1.5 text-sm text-text-muted hover:bg-surface-hover hover:text-text disabled:opacity-40"
          >
            Reset column widths
          </button>
        </>
      )}
    </div>
  );
}

function FieldSelect({
  label,
  hint,
  value,
  options,
  onChange,
}: {
  label: string;
  hint?: string;
  value: string | undefined;
  options: Field[];
  onChange: (id: string | undefined) => void;
}) {
  // A config can name a field that has since been deleted: surface that rather
  // than silently showing the automatic choice.
  const orphaned = !!value && !options.some((f) => f.id === value);
  return (
    <div>
      <label className="mb-1 block text-xs font-medium text-text-faint">
        {label}
      </label>
      <select
        value={orphaned ? "" : (value ?? "")}
        onChange={(e) => onChange(e.target.value || undefined)}
        className={`${controlCls} w-full`}
      >
        <option value="">
          {options.length ? "Automatic (first match)" : "No matching fields"}
        </option>
        {options.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </select>
      {orphaned ? (
        <p className="mt-1 text-2xs text-warning">
          The field this view used was deleted. Choose another.
        </p>
      ) : hint ? (
        <p className="mt-1 text-2xs text-text-faint">{hint}</p>
      ) : null}
    </div>
  );
}

function FilterRow({
  filter,
  field,
  onChange,
  onRemove,
}: {
  filter: Filter;
  field: Field | undefined;
  onChange: (f: Filter) => void;
  onRemove: () => void;
}) {
  // The typed value lives here while it is being edited: writing it straight
  // back to the view config would re-render (and blank) the input mid-word.
  const [draft, setDraft] = useState(String(filter.value ?? ""));
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stored = String(filter.value ?? "");
  const lastStored = useRef(stored);

  useEffect(() => {
    if (stored !== lastStored.current) {
      lastStored.current = stored;
      setDraft(stored);
    }
  }, [stored]);

  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);

  const commit = (value: string) => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    if (value === lastStored.current) return;
    lastStored.current = value;
    onChange({ ...filter, value });
  };

  const type = (e: string) => {
    setDraft(e);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => commit(e), COMMIT_DELAY);
  };

  const setOp = (op: FilterOp) => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    // Ops that take no value drop the stale one so the row reads honestly.
    const value: CellValue = isValueless(op) ? "" : draft;
    lastStored.current = String(value ?? "");
    onChange({ ...filter, op, value });
  };

  if (!field) {
    return (
      <div className="flex items-center gap-1.5">
        <span className="flex-1 truncate text-sm italic text-text-faint">
          Deleted field
        </span>
        <button
          aria-label="Remove filter"
          onClick={onRemove}
          className="grid size-6 shrink-0 place-items-center rounded text-text-faint hover:text-danger-c"
        >
          <X className="size-3.5" />
        </button>
      </div>
    );
  }

  const ops = opsForType(field.type);
  const op = ops.includes(filter.op) ? filter.op : ops[0];
  const needsValue = !isValueless(op);

  return (
    <div className="flex items-center gap-1.5">
      <span className="w-20 shrink-0 truncate text-sm" title={field.name}>
        {field.name}
      </span>
      <select
        aria-label={`${field.name} condition`}
        value={op}
        onChange={(e) => setOp(e.target.value as FilterOp)}
        className={`${controlCls} w-28 shrink-0`}
      >
        {ops.map((o) => (
          <option key={o} value={o}>
            {OP_LABELS[o]}
          </option>
        ))}
      </select>

      {needsValue &&
        (field.type === "select" ? (
          <select
            aria-label={`${field.name} value`}
            value={draft}
            onChange={(e) => commit(e.target.value)}
            className={`${controlCls} flex-1`}
          >
            <option value="">–</option>
            {field.options?.choices?.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        ) : (
          <input
            aria-label={`${field.name} value`}
            type={
              field.type === "number"
                ? "number"
                : field.type === "date"
                  ? "date"
                  : "text"
            }
            value={draft}
            placeholder="value"
            onChange={(e) => type(e.target.value)}
            onBlur={() => commit(draft)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit(draft);
            }}
            className={`${controlCls} flex-1`}
          />
        ))}

      <button
        aria-label="Remove filter"
        onClick={onRemove}
        className="grid size-6 shrink-0 place-items-center rounded text-text-faint hover:text-danger-c"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

function AddControl({
  fields,
  label,
  onPick,
}: {
  fields: Field[];
  label: string;
  onPick: (field: Field) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="mt-2 flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-note text-text-muted hover:bg-surface-hover">
        <Plus className="size-3.5" /> {label}
      </PopoverTrigger>
      <PopoverContent className="max-h-64 w-44 overflow-auto p-1">
        {fields.length === 0 ? (
          <p className="px-2 py-1.5 text-sm text-text-faint">No fields yet.</p>
        ) : (
          fields.map((f) => (
            <button
              key={f.id}
              onClick={() => {
                onPick(f);
                setOpen(false);
              }}
              className="block w-full truncate rounded px-2 py-1.5 text-left text-sm hover:bg-surface-hover"
            >
              {f.name}
            </button>
          ))
        )}
      </PopoverContent>
    </Popover>
  );
}
