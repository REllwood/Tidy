import { Check, CalendarClock } from "lucide-react";
import { dueLabel, snoozeDate, type AgendaTask, type Snooze } from "@/lib/agenda";
import { useTaskMutations } from "@/hooks/useTaskMutations";
import { useUi } from "@/store/ui";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

const STATUS_DOT: Record<string, string> = {
  grey: "var(--text-faint)",
  blue: "var(--info)",
  green: "var(--success)",
  amber: "var(--warning)",
  red: "var(--danger-c)",
};

const SNOOZE: { key: Snooze; label: string }[] = [
  { key: "today", label: "Today" },
  { key: "tomorrow", label: "Tomorrow" },
  { key: "weekend", label: "This weekend" },
  { key: "nextweek", label: "Next week" },
];

/** One actionable planner task: complete, reschedule, or open its source. */
export function TaskRow({
  task,
  today,
  overdue,
  showDbChip = true,
}: {
  task: AgendaTask;
  today: string;
  overdue?: boolean;
  showDbChip?: boolean;
}) {
  const openPage = useUi((s) => s.openPage);
  const { complete, reschedule, canComplete } = useTaskMutations();
  const dot = STATUS_DOT[task.statusColor ?? "grey"] ?? STATUS_DOT.grey;
  const completable = canComplete(task);

  return (
    <div className="group flex items-center gap-2.5 px-3 py-2.5">
      {/* complete toggle (falls back to a status dot when there's no Status field) */}
      {completable ? (
        <button
          aria-label={task.done ? "Mark not done" : "Mark done"}
          onClick={() => complete(task)}
          className={`grid size-[18px] shrink-0 place-items-center rounded-full border transition-colors ${
            task.done
              ? "border-brand bg-brand text-white"
              : "border-border-strong text-transparent hover:border-brand hover:text-brand/40"
          }`}
        >
          <Check className="size-3" />
        </button>
      ) : (
        <span
          className="size-2.5 shrink-0 rounded-full"
          style={{ background: dot }}
          aria-hidden
        />
      )}

      <button
        onClick={() => openPage(task.databasePageId)}
        className={`min-w-0 flex-1 truncate text-left text-sm transition-colors hover:text-brand ${
          task.done ? "text-text-faint line-through" : "text-text"
        }`}
      >
        {task.title}
      </button>

      {showDbChip && (
        <span className="hidden shrink-0 items-center gap-1 rounded-md bg-bg-subtle px-1.5 py-0.5 text-2xs text-text-faint sm:inline-flex">
          {task.databaseTitle}
        </span>
      )}

      {/* due chip → snooze/reschedule menu */}
      <Popover>
        <PopoverTrigger
          className={`shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium transition-colors hover:bg-surface-hover ${
            overdue ? "text-danger-c" : "text-text-muted"
          }`}
        >
          {dueLabel(task.due, today)}
        </PopoverTrigger>
        <PopoverContent align="end" className="w-44 p-1">
          <div className="px-2 pb-1 pt-1 text-2xs font-semibold uppercase tracking-wide text-text-faint">
            Reschedule
          </div>
          {SNOOZE.map((s) => (
            <button
              key={s.key}
              onClick={() => reschedule(task, snoozeDate(s.key))}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-surface-hover"
            >
              <CalendarClock className="size-3.5 text-text-faint" />
              {s.label}
            </button>
          ))}
          <label className="mt-1 flex items-center gap-2 border-t border-border px-2 py-1.5 text-sm text-text-muted">
            <span className="text-xs text-text-faint">Pick</span>
            <input
              type="date"
              defaultValue={task.due}
              onChange={(e) => e.target.value && reschedule(task, e.target.value)}
              className="min-w-0 flex-1 bg-transparent text-sm outline-none"
            />
          </label>
        </PopoverContent>
      </Popover>
    </div>
  );
}
