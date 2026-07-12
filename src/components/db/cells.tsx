import type { Field, SelectChoice, CellValue } from "@/lib/api";

const CHIP_COLORS: Record<string, string> = {
  grey: "color-mix(in srgb, var(--text-muted) 16%, transparent)",
  blue: "color-mix(in srgb, #3b6fb0 18%, transparent)",
  green: "color-mix(in srgb, var(--success) 18%, transparent)",
  amber: "color-mix(in srgb, var(--warning) 20%, transparent)",
  red: "color-mix(in srgb, var(--danger-c) 18%, transparent)",
};
const CHIP_TEXT: Record<string, string> = {
  grey: "var(--text-muted)",
  blue: "#3b6fb0",
  green: "var(--success)",
  amber: "var(--warning)",
  red: "var(--danger-c)",
};

export function choiceById(
  field: Field,
  id: CellValue,
): SelectChoice | undefined {
  return field.options?.choices?.find((c) => c.id === id);
}

export function SelectChip({ choice }: { choice: SelectChoice }) {
  const bg = CHIP_COLORS[choice.color] ?? CHIP_COLORS.grey;
  const fg = CHIP_TEXT[choice.color] ?? CHIP_TEXT.grey;
  return (
    <span
      className="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-semibold"
      style={{ background: bg, color: fg }}
    >
      {choice.name}
    </span>
  );
}

/** Format a derived value (lookup/rollup/formula) for display. */
function formatComputed(value: CellValue): string {
  if (value == null || value === "") return "–";
  if (typeof value === "number") {
    if (!isFinite(value)) return "–";
    return Number.isInteger(value) ? String(value) : value.toFixed(2);
  }
  return String(value);
}

/** Read-only display of a computed cell (lookup / rollup / formula). */
export function ComputedCell({ value }: { field: Field; value: CellValue }) {
  const text = formatComputed(value);
  return (
    <span
      className="truncate text-sm text-text-muted"
      title={text}
      data-computed="true"
    >
      {text}
    </span>
  );
}

/** Read-only rendering of a cell value, used on board cards. */
export function CellDisplay({
  field,
  value,
}: {
  field: Field;
  value: CellValue;
}) {
  if (value === undefined || value === null || value === "")
    return <span className="text-text-faint">–</span>;
  switch (field.type) {
    case "checkbox":
      return <span>{value ? "☑" : "☐"}</span>;
    case "select": {
      const c = choiceById(field, value);
      return c ? <SelectChip choice={c} /> : <span>–</span>;
    }
    case "date":
      return (
        <span className="text-text-muted">
          {new Date(value).toLocaleDateString(undefined, {
            month: "short",
            day: "numeric",
          })}
        </span>
      );
    default:
      return <span>{String(value)}</span>;
  }
}
