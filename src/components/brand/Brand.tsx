import { cn } from "@/lib/utils";

/** The same production mark is used in the app, Dock and browser tab. */
export function Brand({
  compact = false,
  className,
}: {
  compact?: boolean;
  className?: string;
}) {
  return (
    <span className={cn("inline-flex items-center gap-2.5", className)}>
      <img
        src="/tidy-app-icon.png"
        alt={compact ? "Tidy" : ""}
        width={40}
        height={40}
        className="size-10 shrink-0"
        draggable={false}
      />
      {!compact && (
        <span className="text-[24px] font-semibold leading-none tracking-[-0.065em]">
          tidy<span className="text-brand">.</span>
        </span>
      )}
    </span>
  );
}
