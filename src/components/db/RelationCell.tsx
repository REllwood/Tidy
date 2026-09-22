import { useEffect, useState } from "react";
import { Check, Loader2 } from "lucide-react";
import type { DatabaseBundle, Field, CellValue, RowWithCells } from "@/lib/api";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { asIds, rowLabel } from "./viewFields";

/** True once `active` has stayed true for `ms`, so waiting states can give up. */
function useStalled(active: boolean, ms: number): boolean {
  const [stalled, setStalled] = useState(false);
  useEffect(() => {
    if (!active) {
      setStalled(false);
      return;
    }
    const t = setTimeout(() => setStalled(true), ms);
    return () => clearTimeout(t);
  }, [active, ms]);
  return stalled;
}

/**
 * Editable relation cell. `value` is an array of target row ids; `target` is
 * the prefetched bundle of the related database (undefined while it loads).
 */
export function RelationCell({
  field,
  value,
  target,
  onChange,
}: {
  field: Field;
  value: CellValue;
  target: DatabaseBundle | undefined;
  onChange: (v: CellValue) => void;
}) {
  const [open, setOpen] = useState(false);
  const ids = asIds(value);
  const multi = field.options?.multi !== false; // default to multi-select
  const configured = !!field.options?.targetDatabaseId;
  const loading = configured && !target;
  const stalled = useStalled(loading, 6000);
  const selected = ids
    .map((id) => target?.rows.find((r) => r.id === id))
    .filter(Boolean) as RowWithCells[];

  const toggle = (rowId: string) => {
    const next = ids.includes(rowId)
      ? ids.filter((x) => x !== rowId)
      : multi
        ? [...ids, rowId]
        : [rowId];
    onChange(next);
    if (!multi) setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        aria-label={field.name}
        className="flex w-full flex-wrap items-center gap-1 overflow-hidden text-left outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
      >
        {!configured ? (
          <span className="text-sm text-text-faint">Not linked</span>
        ) : selected.length ? (
          selected.map((r) => (
            <span
              key={r.id}
              className="inline-flex max-w-full items-center truncate rounded-full bg-brand-soft px-2 py-0.5 text-xs font-medium text-text"
            >
              {rowLabel(target, r)}
            </span>
          ))
        ) : ids.length ? (
          <span className="text-sm text-text-faint">{ids.length} linked</span>
        ) : (
          <span className="text-sm text-text-faint">Empty</span>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-60 p-1">
        {!configured ? (
          <p className="px-2 py-1.5 text-sm text-text-muted">
            This field isn't pointed at a database yet. Pick one with Edit
            options in the column menu.
          </p>
        ) : loading ? (
          stalled ? (
            <p className="px-2 py-1.5 text-sm text-text-muted">
              The related records are taking a while. Close this and try again.
            </p>
          ) : (
            <div className="flex items-center gap-2 px-2 py-1.5 text-sm text-text-faint">
              <Loader2 className="size-3.5 animate-spin" />
              Loading…
            </div>
          )
        ) : target!.rows.length === 0 ? (
          <p className="px-2 py-1.5 text-sm text-text-faint">No records</p>
        ) : (
          <div className="max-h-64 overflow-auto">
            {target!.rows.map((r) => {
              const on = ids.includes(r.id);
              return (
                <button
                  key={r.id}
                  onClick={() => toggle(r.id)}
                  className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-surface-hover"
                >
                  <span
                    className={`grid size-4 shrink-0 place-items-center rounded border ${
                      on ? "border-brand bg-brand text-white" : "border-border"
                    }`}
                  >
                    {on && <Check className="size-3" />}
                  </span>
                  <span className="truncate">{rowLabel(target, r)}</span>
                </button>
              );
            })}
          </div>
        )}
        {ids.length > 0 && (
          <button
            onClick={() => {
              onChange([]);
              setOpen(false);
            }}
            className="mt-1 w-full rounded px-2 py-1 text-left text-sm text-text-muted hover:bg-surface-hover"
          >
            Clear
          </button>
        )}
      </PopoverContent>
    </Popover>
  );
}
