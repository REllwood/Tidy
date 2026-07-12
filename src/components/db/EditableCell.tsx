import { useEffect, useRef, useState } from "react";
import type { Field, CellValue } from "@/lib/api";
import { choiceById, SelectChip } from "./cells";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

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
      return (
        <input
          type="date"
          value={value ?? ""}
          onChange={(e) => onChange(e.target.value || null)}
          className="w-full bg-transparent text-sm text-text-muted outline-none"
        />
      );
    case "number":
      return (
        <TextLike
          value={value ?? ""}
          numeric
          onCommit={(v) => onChange(v === "" ? null : Number(v))}
          placeholder=""
        />
      );
    default:
      return (
        <TextLike
          value={value ?? ""}
          onCommit={(v) => onChange(v === "" ? null : v)}
          placeholder=""
        />
      );
  }
}

function TextLike({
  value,
  onCommit,
  numeric,
  placeholder,
}: {
  value: CellValue;
  onCommit: (v: string) => void;
  numeric?: boolean;
  placeholder?: string;
}) {
  const [draft, setDraft] = useState(String(value ?? ""));
  const ref = useRef(String(value ?? ""));
  useEffect(() => {
    if (String(value ?? "") !== ref.current) {
      ref.current = String(value ?? "");
      setDraft(ref.current);
    }
  }, [value]);
  return (
    <input
      type={numeric ? "number" : "text"}
      value={draft}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (draft !== String(value ?? "")) onCommit(draft);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
      className="w-full bg-transparent text-sm outline-none placeholder:text-text-faint"
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
  const choice = choiceById(field, value);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="flex w-full items-center text-left outline-none">
        {choice ? (
          <SelectChip choice={choice} />
        ) : (
          <span className="text-sm text-text-faint">Empty</span>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-48 p-1">
        {(field.options?.choices ?? []).map((c) => (
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
        ))}
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
