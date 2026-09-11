export type ModelRange = "all" | "light" | "everyday" | "power";
export const rangeLabels: Record<ModelRange, string> = {
  all: "All models",
  light: "Lower memory",
  everyday: "Everyday use",
  power: "Larger models",
};
export function ModelRangeFilter({ value, onChange, label }: {
  value: ModelRange; onChange: (value: ModelRange) => void; label: string;
}) {
  return (
    <label className="flex flex-wrap items-center gap-2 text-xs text-text-muted">
      {label}
      <select
        value={value}
        onChange={(e) => onChange(e.target.value as ModelRange)}
        className="rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
      >
        {Object.entries(rangeLabels).map(([id, name]) => <option key={id} value={id}>{name}</option>)}
      </select>
    </label>
  );
}
export function MemoryGuidance({ suggested, memoryBytes }: { suggested: number; memoryBytes?: number | null }) {
  const below = memoryBytes != null && memoryBytes < suggested * 1024 ** 3;
  return <p className="mt-1 text-xs leading-relaxed text-text-faint">
    Suggested system RAM: {suggested} GB or more.
    {below && " This Mac has less RAM than suggested; processing may be slow or fail. You can still choose this model."}
  </p>;
}
