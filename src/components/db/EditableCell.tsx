import { useEffect, useRef, useState } from "react";
import type { Field, CellValue } from "@/lib/api";
import { choiceById, SelectChip } from "./cells";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

const FOCUSABLE =
  'input:not([disabled]),select:not([disabled]),textarea:not([disabled]),button:not([disabled]),[tabindex]:not([tabindex="-1"])';

/**
 * Move focus to the next (or previous) editable cell in the same row, so Tab
 * walks across a record instead of falling into the row's action buttons.
 * Returns false when there is nothing to move to, leaving Tab to the browser.
 */
export function focusAdjacentCell(from: HTMLElement, dir: 1 | -1): boolean {
  const cell = from.closest<HTMLElement>("[data-cell]");
  const row = cell?.closest<HTMLElement>("[data-row]");
  if (!cell || !row) return false;
  const cells = Array.from(row.querySelectorAll<HTMLElement>("[data-cell]"));
  for (let i = cells.indexOf(cell) + dir; i >= 0 && i < cells.length; i += dir) {
    const control = cells[i].querySelector<HTMLElement>(FOCUSABLE);
    if (!control) continue;
    control.focus();
    if (control instanceof HTMLInputElement && control.type === "text")
      control.select();
    return true;
  }
  return false;
}

/** Tab/Shift+Tab across the row; returns true when it handled the key. */
function handleTab(e: React.KeyboardEvent<HTMLElement>): boolean {
  if (e.key !== "Tab") return false;
  if (e.altKey || e.metaKey || e.ctrlKey) return false;
  if (!focusAdjacentCell(e.currentTarget, e.shiftKey ? -1 : 1)) return false;
  e.preventDefault();
  return true;
}

export function EditableCell({
  field,
  value,
  onChange,
}: {
  field: Field;
  value: CellValue;
  onChange: (v: CellValue) => void;
}) {
  switch (field.type) {
    case "checkbox":
      return (
        <input
          type="checkbox"
          checked={!!value}
          onChange={(e) => onChange(e.target.checked)}
          onKeyDown={handleTab}
          className="size-4 accent-[var(--brand)]"
          aria-label={field.name}
        />
      );
    case "select":
      return <SelectCell field={field} value={value} onChange={onChange} />;
    case "dependencies":
      return (
        <span className="text-sm text-text-faint">
          {(Array.isArray(value) ? value.length : 0) || 0} linked
        </span>
      );
    case "date":
      return <DateLike field={field} value={value} onChange={onChange} />;
    case "number":
      return (
        <TextLike
          value={value ?? ""}
          label={field.name}
          numeric
          onCommit={(v) => onChange(v === "" ? null : Number(v))}
          placeholder=""
        />
      );
    default:
      return (
        <TextLike
          value={value ?? ""}
          label={field.name}
          onCommit={(v) => onChange(v === "" ? null : v)}
          placeholder=""
        />
      );
  }
}

/** Keep a local draft in step with the stored value without clobbering typing. */
function useDraft(value: CellValue) {
  const text = String(value ?? "");
  const [draft, setDraft] = useState(text);
  const last = useRef(text);
  useEffect(() => {
    if (text !== last.current) {
      last.current = text;
      setDraft(text);
    }
  }, [text]);
  return { draft, setDraft, last, text };
}

function TextLike({
  value,
  onCommit,
  numeric,
  placeholder,
  label,
}: {
  value: CellValue;
  onCommit: (v: string) => void;
  numeric?: boolean;
  placeholder?: string;
  label?: string;
}) {
  const { draft, setDraft, last, text } = useDraft(value);
  const cancelled = useRef(false);

  const commit = (next: string) => {
    if (next === text) return;
    last.current = next;
    onCommit(next);
  };

  return (
    <input
      type={numeric ? "number" : "text"}
      value={draft}
      aria-label={label}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        // Escape already restored the draft: leave the stored value alone.
        if (cancelled.current) {
          cancelled.current = false;
          return;
        }
        commit(draft);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          cancelled.current = true;
          setDraft(text);
          e.currentTarget.blur();
        } else {
          handleTab(e);
        }
      }}
      className="w-full bg-transparent text-sm outline-none placeholder:text-text-faint"
    />
  );
}

/**
 * Date cell. A native date input reports an empty value while its segments are
 * half filled (`validity.badInput`), so writing on every keystroke would wipe
 * the stored date mid-edit. The draft is local and only committed once the
 * input holds a complete date, or on blur/Enter when it is genuinely empty.
 */
function DateLike({
  field,
  value,
  onChange,
}: {
  field: Field;
  value: CellValue;
  onChange: (v: CellValue) => void;
}) {
  const { draft, setDraft, last, text } = useDraft(value);
  const cancelled = useRef(false);

  const commitFrom = (el: HTMLInputElement) => {
    if (el.validity?.badInput) return; // still half typed
    const next = el.value;
    if (next === text) return;
    last.current = next;
    onChange(next === "" ? null : next);
  };

  return (
    <input
      type="date"
      value={draft}
      aria-label={field.name}
      onChange={(e) => {
        const el = e.currentTarget;
        setDraft(el.value);
        // A complete date (picked or fully typed) can be written straight away;
        // an empty one waits for blur so clearing is deliberate.
        if (el.value) commitFrom(el);
      }}
      onBlur={(e) => {
        if (cancelled.current) {
          cancelled.current = false;
          return;
        }
        commitFrom(e.currentTarget);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commitFrom(e.currentTarget);
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          cancelled.current = true;
          setDraft(text);
          e.currentTarget.blur();
        } else {
          handleTab(e);
        }
      }}
      className="w-full bg-transparent text-sm text-text-muted outline-none"
    />
  );
}

function SelectCell({
  field,
  value,
  onChange,
}: {
  field: Field;
  value: CellValue;
  onChange: (v: CellValue) => void;
}) {
  const [open, setOpen] = useState(false);
  const choices = field.options?.choices ?? [];
  const choice = choiceById(field, value);

  // If the choices disappear underneath an open picker, close it.
  useEffect(() => {
    if (open && choices.length === 0) setOpen(false);
  }, [open, choices.length]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        aria-label={field.name}
        onKeyDown={(e) => {
          if (!open) handleTab(e);
        }}
        className="flex w-full items-center text-left outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
      >
        {choice ? (
          <SelectChip choice={choice} />
        ) : (
          <span className="text-sm text-text-faint">Empty</span>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-48 p-1">
        {choices.length === 0 ? (
          <p className="px-2 py-1.5 text-sm text-text-faint">
            No choices yet. Add some from the column menu.
          </p>
        ) : (
          choices.map((c) => (
            <button
              key={c.id}
              onClick={() => {
                onChange(c.id);
                setOpen(false);
              }}
              className="flex w-full items-center rounded px-2 py-1.5 hover:bg-surface-hover"
            >
              <SelectChip choice={c} />
            </button>
          ))
        )}
        {value != null && (
          <button
            onClick={() => {
              onChange(null);
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
