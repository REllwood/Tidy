import { useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  CalendarCheck,
  Plus,
  AlertTriangle,
  Sun,
  CalendarDays,
  CalendarClock,
  Check,
  ChevronDown,
  X,
} from "lucide-react";
import { databasesApi } from "@/lib/api";
import { useAgenda, type PlannerDb } from "@/hooks/useAgenda";
import { TaskRow } from "@/components/planner/TaskRow";
import type { AgendaTask } from "@/lib/agenda";
import { Skeleton } from "@/components/ui/skeleton";

export function PlannerView() {
  const { agenda, done, tasks, databases, today, loading } = useAgenda();
  const [filter, setFilter] = useState<string | null>(null);
  const [day, setDay] = useState<string | null>(null);
  const [showDone, setShowDone] = useState(false);

  const match = (t: AgendaTask) =>
    (!filter || t.databaseId === filter) && (!day || t.due === day);

  const sections = [
    { key: "overdue", label: "Overdue", overdue: true, icon: <AlertTriangle className="size-3.5 text-danger-c" />, items: agenda.overdue.filter(match) },
    { key: "today", label: "Today", icon: <Sun className="size-3.5 text-brand" />, items: agenda.today.filter(match) },
    { key: "week", label: "This week", icon: <CalendarDays className="size-3.5 text-text-faint" />, items: agenda.week.filter(match) },
    { key: "later", label: "Later", icon: <CalendarClock className="size-3.5 text-text-faint" />, items: agenda.later.filter(match) },
  ];
  const openCount = sections.reduce((n, s) => n + s.items.length, 0);
  const doneItems = done.filter(match);
  const addable = databases.filter((d) => d.nameFieldId && d.dueFieldId);

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto grid max-w-5xl gap-8 px-10 py-10 lg:grid-cols-[1fr_248px]">
        <div className="min-w-0">
          <header className="mb-6">
            <div className="mb-1 flex items-center gap-2 text-note font-medium text-text-faint">
              <CalendarCheck className="size-3.5" /> Planner
            </div>
            <h1 className="text-2xl font-bold tracking-tight">Your tasks</h1>
            <p className="mt-1 text-sm text-text-muted">
              {openCount} open{" "}
              {agenda.overdue.length > 0 && (
                <span className="text-danger-c">· {agenda.overdue.length} overdue</span>
              )}
            </p>
          </header>

          {addable.length > 0 && <QuickAdd databases={addable} today={today} />}

          {databases.length > 1 && (
            <div className="mb-5 flex flex-wrap gap-1.5">
              <FilterChip label="All" active={!filter} onClick={() => setFilter(null)} />
              {databases.map((d) => (
                <FilterChip
                  key={d.id}
                  label={d.title}
                  active={filter === d.id}
                  onClick={() => setFilter(filter === d.id ? null : d.id)}
                />
              ))}
            </div>
          )}

          {day && (
            <div className="mb-4 flex items-center gap-2 rounded-lg bg-brand-soft px-3 py-1.5 text-note text-text">
              Showing{" "}
              <b>
                {new Date(`${day}T00:00:00`).toLocaleDateString(undefined, {
                  weekday: "long",
                  month: "long",
                  day: "numeric",
                })}
              </b>
              <button
                onClick={() => setDay(null)}
                className="ml-auto grid size-5 place-items-center rounded hover:bg-surface-hover"
                aria-label="Clear day filter"
              >
                <X className="size-3.5" />
              </button>
            </div>
          )}

          {loading && tasks.length === 0 ? (
            <div className="space-y-2">
              <Skeleton className="h-11 w-full" />
              <Skeleton className="h-11 w-full" />
            </div>
          ) : openCount === 0 ? (
            <div className="flex flex-col items-center rounded-[var(--radius-lg)] border border-dashed border-border bg-surface/50 px-6 py-14 text-center">
              <span className="mb-2 grid size-11 place-items-center rounded-full bg-brand-soft text-brand">
                <Check className="size-5" />
              </span>
              <div className="text-sm font-semibold">Nothing to do here</div>
              <p className="mt-1 text-note text-text-muted">
                {day || filter ? "No tasks match this filter." : "You're all caught up. Add a task above."}
              </p>
            </div>
          ) : (
            <div className="space-y-5">
              {sections.map(
                (s) =>
                  s.items.length > 0 && (
                    <Section
                      key={s.key}
                      label={s.label}
                      icon={s.icon}
                      count={s.items.length}
                      danger={s.overdue}
                    >
                      {s.items.map((t) => (
                        <TaskRow key={t.rowId} task={t} today={today} overdue={s.overdue} />
                      ))}
                    </Section>
                  ),
              )}
            </div>
          )}

          {doneItems.length > 0 && (
            <div className="mt-6">
              <button
                onClick={() => setShowDone((v) => !v)}
                className="flex items-center gap-1.5 rounded-md px-1 py-1 text-xs font-semibold text-text-faint hover:text-text"
              >
                <ChevronDown
                  className={`size-3.5 transition-transform ${showDone ? "" : "-rotate-90"}`}
                />
                Completed · {doneItems.length}
              </button>
              {showDone && (
                <div className="mt-1.5 divide-y divide-border/60 overflow-hidden rounded-[var(--radius-lg)] border border-border bg-surface opacity-80">
                  {doneItems.slice(0, 50).map((t) => (
                    <TaskRow key={t.rowId} task={t} today={today} />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        <aside className="hidden lg:block">
          <MiniCalendar tasks={tasks} today={today} selected={day} onSelect={setDay} />
        </aside>
      </div>
    </div>
  );
}

function Section({
  label,
  icon,
  count,
  danger,
  children,
}: {
  label: string;
  icon: React.ReactNode;
  count: number;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-1.5 flex items-center gap-1.5 px-1">
        {icon}
        <span className={`text-xs font-semibold ${danger ? "text-danger-c" : "text-text-muted"}`}>
          {label}
        </span>
        <span className="text-xs text-text-faint">· {count}</span>
      </div>
      <div className="divide-y divide-border/60 overflow-hidden rounded-[var(--radius-lg)] border border-border bg-surface">
        {children}
      </div>
    </div>
  );
}

function FilterChip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded-full border px-2.5 py-1 text-xs font-medium transition-colors ${
        active
          ? "border-brand/40 bg-brand-soft text-text"
          : "border-border text-text-muted hover:bg-surface-hover hover:text-text"
      }`}
    >
      {label}
    </button>
  );
}

function QuickAdd({ databases, today }: { databases: PlannerDb[]; today: string }) {
  const [title, setTitle] = useState("");
  const [dbId, setDbId] = useState(databases[0]?.id ?? "");
  const qc = useQueryClient();
  const add = useMutation({
    mutationFn: async () => {
      const db = databases.find((d) => d.id === dbId) ?? databases[0];
      if (!db) return;
      const row = await databasesApi.createRow(db.id);
      if (db.nameFieldId) await databasesApi.setCell(row.id, db.nameFieldId, title.trim());
      if (db.dueFieldId) await databasesApi.setCell(row.id, db.dueFieldId, today);
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["database-by-id"] });
      qc.invalidateQueries({ queryKey: ["database"] });
      setTitle("");
    },
  });

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (title.trim() && !add.isPending) add.mutate();
      }}
      className="mb-5 flex items-center gap-2 rounded-[var(--radius-lg)] border border-border bg-surface px-3 py-2 focus-within:border-brand"
    >
      <Plus className="size-4 shrink-0 text-text-faint" />
      <input
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder="Add a task for today…"
        className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-text-faint"
      />
      {databases.length > 1 && (
        <select
          value={dbId}
          onChange={(e) => setDbId(e.target.value)}
          className="shrink-0 rounded-md border border-border bg-bg-subtle px-1.5 py-1 text-xs text-text-muted outline-none"
        >
          {databases.map((d) => (
            <option key={d.id} value={d.id}>
              {d.title}
            </option>
          ))}
        </select>
      )}
      <button
        type="submit"
        disabled={!title.trim() || add.isPending}
        className="shrink-0 rounded-md bg-brand px-2.5 py-1 text-note font-medium text-white hover:bg-brand/90 disabled:opacity-40"
      >
        Add
      </button>
    </form>
  );
}

function MiniCalendar({
  tasks,
  today,
  selected,
  onSelect,
}: {
  tasks: AgendaTask[];
  today: string;
  selected: string | null;
  onSelect: (iso: string | null) => void;
}) {
  const base = new Date(`${today}T12:00:00`);
  const year = base.getFullYear();
  const month = base.getMonth();
  const startDay = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const iso = (d: number) =>
    `${year}-${String(month + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;

  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const t of tasks) if (!t.done) c[t.due] = (c[t.due] ?? 0) + 1;
    return c;
  }, [tasks]);

  const cells: (number | null)[] = [
    ...Array(startDay).fill(null),
    ...Array.from({ length: daysInMonth }, (_, i) => i + 1),
  ];

  return (
    <div className="sticky top-4 rounded-[var(--radius-lg)] border border-border bg-surface p-3">
      <div className="mb-2 text-center text-note font-semibold">
        {base.toLocaleDateString(undefined, { month: "long", year: "numeric" })}
      </div>
      <div className="grid grid-cols-7 gap-0.5 text-center text-[10px] font-medium text-text-faint">
        {["S", "M", "T", "W", "T", "F", "S"].map((d, i) => (
          <div key={i}>{d}</div>
        ))}
      </div>
      <div className="mt-1 grid grid-cols-7 gap-0.5">
        {cells.map((d, i) =>
          d === null ? (
            <div key={i} />
          ) : (
            <button
              key={i}
              onClick={() => onSelect(selected === iso(d) ? null : iso(d))}
              className={`relative grid aspect-square place-items-center rounded-md text-xs transition-colors ${
                iso(d) === today
                  ? "bg-brand font-semibold text-white"
                  : iso(d) === selected
                    ? "bg-brand-soft text-text"
                    : "text-text-muted hover:bg-surface-hover"
              }`}
            >
              {d}
              {counts[iso(d)] && iso(d) !== today && (
                <span className="absolute bottom-[3px] size-1 rounded-full bg-brand" />
              )}
            </button>
          ),
        )}
      </div>
      <p className="mt-2 px-1 text-2xs text-text-faint">Click a day to filter tasks.</p>
    </div>
  );
}
