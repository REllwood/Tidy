import { Lightbulb, X } from "lucide-react";
import { useUi } from "@/store/ui";

/** A one-time, dismissible hint. Remembers dismissal in the persisted UI store. */
export function FeatureTip({
  id,
  children,
}: {
  id: string;
  children: React.ReactNode;
}) {
  const dismissed = useUi((s) => s.dismissedTips.includes(id));
  const dismiss = useUi((s) => s.dismissTip);
  if (dismissed) return null;
  return (
    <div className="mb-4 flex items-start gap-2.5 rounded-lg border border-brand/20 bg-brand-soft/50 px-3 py-2 text-note text-text-muted">
      <Lightbulb className="mt-0.5 size-4 shrink-0 text-brand" />
      <div className="min-w-0 flex-1">{children}</div>
      <button
        onClick={() => dismiss(id)}
        aria-label="Dismiss tip"
        className="grid size-5 shrink-0 place-items-center rounded text-text-faint hover:bg-surface-hover hover:text-text"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}
