import { audioLevel } from "./audioLevel";

export function AudioLevelMeter({
  label,
  value,
  available = true,
}: {
  label: string;
  value: number;
  available?: boolean;
}) {
  const level = available ? audioLevel(value) : 0;
  const bars = 16;
  const percent = Math.round(level * 100);
  return (
    <div className="ml-auto flex shrink-0 flex-col items-end gap-1.5">
      <div
        role="meter"
        aria-label={`${label} level`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={
          available
            ? percent
              ? `${percent}% input level`
              : "Quiet"
            : "Unavailable"
        }
        className="flex h-7 items-end gap-0.5"
      >
        {Array.from({ length: bars }, (_, i) => (
          <span
            key={i}
            aria-hidden
            className={`w-[3px] rounded-sm transition-[height,background-color] duration-100 ${level * bars > i ? (i > 13 ? "bg-warning" : "bg-brand") : "bg-border-strong"}`}
            style={{
              height:
                level * bars > i ? `${8 + (i / (bars - 1)) * 20}px` : "4px",
            }}
          />
        ))}
      </div>
      <span className="text-[10px] text-text-faint">
        {!available ? "Unavailable" : level > 0 ? "Receiving audio" : "Quiet"}
      </span>
    </div>
  );
}
