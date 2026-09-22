import { AlertTriangle } from "lucide-react";

/**
 * The shared empty state for every database view: a dashed card with a round
 * brand icon, a one-line heading and a hint, matching the planner's.
 */
export function DbEmptyState({
  icon,
  title,
  hint,
  action,
  className = "",
}: {
  icon: React.ReactNode;
  title: string;
  hint?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`flex flex-col items-center rounded-[var(--radius-lg)] border border-dashed border-border bg-surface/50 px-6 py-14 text-center ${className}`}
    >
      <span className="mb-2 grid size-11 place-items-center rounded-full bg-brand-soft text-brand">
        {icon}
      </span>
      <div className="text-sm font-semibold text-text">{title}</div>
      {hint && <p className="mt-1 text-note text-text-muted">{hint}</p>}
      {action && <div className="mt-3">{action}</div>}
    </div>
  );
}

/** Shown when a view's configured field has been deleted out from under it. */
export function MissingFieldNotice({ className = "" }: { className?: string }) {
  return (
    <div
      role="status"
      className={`flex items-center gap-2 rounded-md border border-border bg-bg-subtle px-3 py-2 text-note text-text-muted ${className}`}
    >
      <AlertTriangle className="size-3.5 shrink-0 text-warning" />
      The field this view used was deleted. Choose another in View settings.
    </div>
  );
}
