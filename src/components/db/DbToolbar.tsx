import { ListFilter, ArrowUpDown, Plus, X } from "lucide-react";
import type { DatabaseBundle, DbView, Field, ViewConfig } from "@/lib/api";
import { defaultOp, type Filter, type Sort } from "@/lib/dbFilters";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

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
          className={`flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-note transition-colors hover:bg-surface-hover ${
            filters.length ? "text-brand" : "text-text-muted"
          }`}
        >
          <ListFilter className="size-3.5" />
          Filter{filters.length ? ` (${filters.length})` : ""}
        </PopoverTrigger>
        <PopoverContent className="w-80 p-2">
          {filters.length === 0 && (
            <p className="px-1 py-1 text-note text-text-faint">No filters.</p>
          )}
          <div className="space-y-1.5">
            {filters.map((f, i) => (
              <FilterRow
                key={i}
                filter={f}
                fields={fields}
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
          className={`flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-note transition-colors hover:bg-surface-hover ${
            sorts.length ? "text-brand" : "text-text-muted"
          }`}
        >
          <ArrowUpDown className="size-3.5" />
          Sort{sorts.length ? ` (${sorts.length})` : ""}
        </PopoverTrigger>
        <PopoverContent className="w-72 p-2">
          {sorts.length === 0 && (
            <p className="px-1 py-1 text-note text-text-faint">No sorts.</p>
          )}
          <div className="space-y-1.5">
            {sorts.map((s, i) => (
              <div key={i} className="flex items-center gap-1.5">
                <span className="flex-1 truncate text-sm">
                  {fields.find((f) => f.id === s.fieldId)?.name ?? "?"}
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
            ))}
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
    </div>
  );
}

function FilterRow({
  filter,
  fields,
  onChange,
  onRemove,
}: {
  filter: Filter;
  fields: Field[];
  onChange: (f: Filter) => void;
  onRemove: () => void;
}) {
  const field = fields.find((f) => f.id === filter.fieldId);
  return (
    <div className="flex items-center gap-1.5">
      <span className="w-20 shrink-0 truncate text-sm">{field?.name ?? "?"}</span>
      {field?.type === "select" ? (
        <select
          value={String(filter.value ?? "")}
          onChange={(e) => onChange({ ...filter, value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-border bg-surface px-1.5 py-1 text-sm"
        >
          <option value="">–</option>
          {field.options?.choices?.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      ) : field?.type === "checkbox" ? (
        <select
          value={filter.op}
          onChange={(e) =>
            onChange({ ...filter, op: e.target.value as Filter["op"] })
          }
          className="min-w-0 flex-1 rounded border border-border bg-surface px-1.5 py-1 text-sm"
        >
          <option value="checked">Checked</option>
          <option value="unchecked">Unchecked</option>
        </select>
      ) : (
        <input
          type={field?.type === "number" ? "number" : field?.type === "date" ? "date" : "text"}
          value={String(filter.value ?? "")}
          placeholder="value"
          onChange={(e) => onChange({ ...filter, value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-border bg-surface px-1.5 py-1 text-sm"
        />
      )}
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
  return (
    <Popover>
      <PopoverTrigger className="mt-2 flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-note text-text-muted hover:bg-surface-hover">
        <Plus className="size-3.5" /> {label}
      </PopoverTrigger>
      <PopoverContent className="w-44 p-1">
        {fields.map((f) => (
          <button
            key={f.id}
            onClick={() => onPick(f)}
            className="block w-full rounded px-2 py-1.5 text-left text-sm hover:bg-surface-hover"
          >
            {f.name}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  );
}
