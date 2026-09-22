import type { Field, SelectChoice, CellValue } from "@/lib/api";
import { parseLocalDate } from "@/lib/dates";

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

/** The colours a select choice can take, in picker order. */
export const CHIP_PALETTE = ["grey", "blue", "green", "amber", "red"] as const;
export type ChipColor = (typeof CHIP_PALETTE)[number];

export const chipColours = (color: string) => ({
  background: CHIP_COLORS[color] ?? CHIP_COLORS.grey,
  color: CHIP_TEXT[color] ?? CHIP_TEXT.grey,
});

export function choiceById(
  field: Field,
  id: CellValue,
): SelectChoice | undefined {
  return field.options?.choices?.find((c) => c.id === id);
}

export function SelectChip({ choice }: { choice: SelectChoice }) {
  const { background, color } = chipColours(choice.color);
  return (
    <span
      className="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-semibold"
      style={{ background, color }}
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
  if (Array.isArray(value)) return value.length ? value.join(", ") : "–";
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

/** Short local-calendar label for a date cell ("20 Jun"-style, locale aware). */
export function formatDayLabel(value: CellValue): string {
  const d = parseLocalDate(value);
  if (!d) return "–";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
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
      // Date cells are plain "YYYY-MM-DD" strings: parse them as a LOCAL day so
      // cards don't read a day early west of UTC.
      return <span className="text-text-muted">{formatDayLabel(value)}</span>;
    case "lookup":
    case "rollup":
    case "formula":
      return <ComputedCell field={field} value={value} />;
    default:
      return <span className="truncate">{formatComputed(value)}</span>;
  }
}
