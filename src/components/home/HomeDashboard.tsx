import { useMemo } from "react";
import {
  Sparkles,
  FilePlus2,
  Mic,
  Waypoints,
  CalendarClock,
  AlertTriangle,
  Sun,
  CalendarDays,
  Clock,
  Star,
  Table2,
  X,
  ArrowRight,
} from "lucide-react";
import { usePages, useCreatePage } from "@/hooks/usePages";
import { useAgenda } from "@/hooks/useAgenda";
import { useUi } from "@/store/ui";
import { greeting, type AgendaTask, type Bucket } from "@/lib/agenda";
import { TaskRow } from "@/components/planner/TaskRow";
import type { Page } from "@/lib/api";

export function HomeDashboard() {
  const openPage = useUi((s) => s.openPage);
  const setActivePane = useUi((s) => s.setActivePane);
  const setIngestOpen = useUi((s) => s.setIngestOpen);
  const onboarded = useUi((s) => s.onboarded);
  const setOnboarded = useUi((s) => s.setOnboarded);
  const create = useCreatePage();
  const { data: pages = [] } = usePages();
  const { agenda, today } = useAgenda();
  const firstDatabase = pages.find((p) => p.type === "database");

  const now = new Date();
  const dateLine = now.toLocaleDateString(undefined, {
    weekday: "long",
    month: "long",
    day: "numeric",
  });

  const recent = useMemo(
    () =>
      [...pages]
        .filter((p) => p.type !== "database" || true)
        .sort((a, b) => b.updated_at - a.updated_at)
        .slice(0, 6),
    [pages],
  );
  const favorites = useMemo(() => pages.filter((p) => p.is_favorite), [pages]);

  const dueToday = agenda.today.length;
  const overdue = agenda.overdue.length;
  // Home shows overdue / today / this-week (not "later"), count only what renders.
  const openCount = agenda.overdue.length + agenda.today.length + agenda.week.length;

  const summary =
    overdue > 0
      ? `${overdue} overdue · ${dueToday} due today`
      : dueToday > 0
        ? `${dueToday} thing${dueToday === 1 ? "" : "s"} due today`
        : agenda.week.length > 0
          ? `Nothing due today · ${agenda.week.length} this week`
          : "You're all caught up.";

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-10 py-12">
        {!onboarded && (
          <div className="mb-8 rounded-[var(--radius-lg)] border border-brand/25 bg-brand-soft/60 p-5">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-lg font-bold tracking-tight">Welcome to Tidy 👋</h2>
                <p className="mt-1 max-w-md text-sm text-text-muted">
                  Your private, local hub for notes, tasks, tables and meetings.
                  All in one place. Three ways to start:
                </p>
              </div>
              <button
                onClick={() => setOnboarded(true)}
                className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-text-muted hover:bg-surface-hover hover:text-text"
              >
                <X className="size-3.5" /> Dismiss
              </button>
            </div>
            <div className="mt-4 grid gap-2.5 sm:grid-cols-3">
              <StartCard
                icon={<FilePlus2 className="size-4" />}
                title="Write a page"
                detail="Rich notes with a slash menu and [[wiki-links]]."
                onClick={() =>
                  create.mutate({ title: "Untitled" }, { onSuccess: (p) => openPage(p.id) })
                }
              />
              <StartCard
                icon={<Table2 className="size-4" />}
                title="Explore a database"
                detail="Grid, board, calendar and Gantt, all one dataset."
                onClick={() => firstDatabase && openPage(firstDatabase.id)}
                disabled={!firstDatabase}
              />
              <StartCard
                icon={<Sparkles className="size-4" />}
                title="File a note"
                detail="Paste anything. It gets summarised and filed."
                onClick={() => setIngestOpen(true)}
              />
            </div>
          </div>
        )}

        {/* Hero */}
        <div className="mb-8">
          <div className="mb-1.5 flex items-center gap-2 text-note font-medium text-text-faint">
            <CalendarDays className="size-3.5" />
            {dateLine}
          </div>
          <h1 className="text-3xl font-bold tracking-tight text-text">
            {greeting(now)}
          </h1>
          <p className="mt-1.5 text-md text-text-muted">{summary}</p>
        </div>

        {/* Quick actions */}
        <div className="mb-10 grid grid-cols-2 gap-2.5 sm:grid-cols-4">
          <QuickAction
            icon={<Sparkles className="size-[18px]" />}
            label="File a note"
            hint="Summarise & file"
            onClick={() => setIngestOpen(true)}
            accent
          />
          <QuickAction
            icon={<FilePlus2 className="size-[18px]" />}
            label="New page"
            hint="Blank doc"
            onClick={() =>
              create.mutate({ title: "Untitled" }, { onSuccess: (p) => openPage(p.id) })
            }
          />
          <QuickAction
            icon={<Mic className="size-[18px]" />}
            label="Record"
            hint="Meeting notes"
            onClick={() => setActivePane({ kind: "meeting" })}
          />
          <QuickAction
            icon={<Waypoints className="size-[18px]" />}
            label="Graph"
            hint="See connections"
            onClick={() => setActivePane({ kind: "graph" })}
          />
        </div>

        {/* Agenda */}
        <section className="mb-10">
          <SectionHeader
            icon={<CalendarClock className="size-4" />}
            title="Your day"
            count={openCount}
          />
          {openCount === 0 ? (
            <EmptyCard
              icon={<Sun className="size-5" />}
              title="No tasks scheduled"
              detail="Add dates to rows in a database and they'll show up here."
            />
          ) : (
            <div className="space-y-5">
              <TaskGroup
                bucket="overdue"
                label="Overdue"
                icon={<AlertTriangle className="size-3.5 text-danger-c" />}
                tasks={agenda.overdue}
                today={today}
              />
              <TaskGroup
                bucket="today"
                label="Today"
                icon={<Sun className="size-3.5 text-brand" />}
                tasks={agenda.today}
                today={today}
              />
              <TaskGroup
                bucket="week"
                label="This week"
                icon={<CalendarDays className="size-3.5 text-text-faint" />}
                tasks={agenda.week}
                today={today}
              />
            </div>
          )}
        </section>

        {/* Recent + Favorites */}
        <div className="grid gap-8 sm:grid-cols-2">
          <section>
            <SectionHeader icon={<Clock className="size-4" />} title="Recently edited" />
            <div className="space-y-0.5">
              {recent.length === 0 ? (
                <p className="px-1 text-sm text-text-faint">Nothing yet.</p>
              ) : (
                recent.map((p) => (
                  <PageRow key={p.id} page={p} onOpen={() => openPage(p.id)} />
                ))
              )}
            </div>
          </section>
          <section>
            <SectionHeader icon={<Star className="size-4" />} title="Favorites" />
            <div className="space-y-0.5">
              {favorites.length === 0 ? (
                <p className="px-1 text-sm text-text-faint">
                  Star a page to pin it here.
                </p>
              ) : (
                favorites.map((p) => (
                  <PageRow key={p.id} page={p} onOpen={() => openPage(p.id)} />
                ))
              )}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

function StartCard({
  icon,
  title,
  detail,
  onClick,
  disabled,
}: {
  icon: React.ReactNode;
  title: string;
  detail: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="group flex flex-col gap-1.5 rounded-lg border border-border bg-surface p-3 text-left transition-colors hover:border-brand/40 disabled:opacity-50"
    >
      <span className="flex items-center gap-1.5 text-brand">
        {icon}
        <ArrowRight className="size-3 opacity-0 transition-opacity group-hover:opacity-100" />
      </span>
      <span className="text-note font-semibold text-text">{title}</span>
      <span className="text-2xs leading-snug text-text-faint">{detail}</span>
    </button>
  );
}

function QuickAction({
  icon,
  label,
  hint,
  onClick,
  accent,
}: {
  icon: React.ReactNode;
  label: string;
  hint: string;
  onClick: () => void;
  accent?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={`group flex flex-col gap-2 rounded-[var(--radius-lg)] border p-3.5 text-left transition-[transform,box-shadow,border-color] duration-200 ease-out hover:-translate-y-0.5 hover:shadow-md ${
        accent
          ? "border-brand/30 bg-brand-soft hover:border-brand/50"
          : "border-border bg-surface hover:border-border-strong"
      }`}
    >
      <span
        className={`grid size-9 place-items-center rounded-lg transition-colors ${
          accent
            ? "bg-brand text-white"
            : "bg-bg-subtle text-text-muted group-hover:text-text"
        }`}
      >
        {icon}
      </span>
      <span>
        <span className="block text-note font-semibold text-text">{label}</span>
        <span className="block text-xs text-text-faint">{hint}</span>
      </span>
    </button>
  );
}

function SectionHeader({
  icon,
  title,
  count,
}: {
  icon: React.ReactNode;
  title: string;
  count?: number;
}) {
  return (
    <div className="mb-3 flex items-center gap-2">
      <span className="text-text-faint">{icon}</span>
      <h2 className="text-2xs font-semibold uppercase tracking-wide text-text-faint">
        {title}
      </h2>
      {count != null && count > 0 && (
        <span className="rounded-full bg-surface-hover px-1.5 py-0.5 text-2xs font-semibold text-text-faint">
          {count}
        </span>
      )}
    </div>
  );
}

function TaskGroup({
  bucket,
  label,
  icon,
  tasks,
  today,
}: {
  bucket: Bucket;
  label: string;
  icon: React.ReactNode;
  tasks: AgendaTask[];
  today: string;
}) {
  if (tasks.length === 0) return null;
  return (
    <div>
      <div className="mb-1.5 flex items-center gap-1.5 px-1">
        {icon}
        <span
          className={`text-xs font-semibold ${
            bucket === "overdue" ? "text-danger-c" : "text-text-muted"
          }`}
        >
          {label}
        </span>
        <span className="text-xs text-text-faint">· {tasks.length}</span>
      </div>
      <div className="divide-y divide-border/60 overflow-hidden rounded-[var(--radius-lg)] border border-border bg-surface">
        {tasks.map((t) => (
          <TaskRow key={t.rowId} task={t} today={today} overdue={bucket === "overdue"} />
        ))}
      </div>
    </div>
  );
}

function PageRow({ page, onOpen }: { page: Page; onOpen: () => void }) {
  return (
    <button
      onClick={onOpen}
      className="flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-surface-hover"
    >
      <span className="grid size-6 shrink-0 place-items-center rounded-md bg-bg-subtle text-sm">
        {page.icon ?? (page.type === "database" ? "🗂️" : "📄")}
      </span>
      <span className="min-w-0 flex-1 truncate text-sm text-text">
        {page.title || "Untitled"}
      </span>
    </button>
  );
}

function EmptyCard({
  icon,
  title,
  detail,
}: {
  icon: React.ReactNode;
  title: string;
  detail: string;
}) {
  return (
    <div className="flex flex-col items-center rounded-xl border border-dashed border-border bg-surface/50 px-6 py-10 text-center">
      <span className="mb-2 grid size-10 place-items-center rounded-full bg-brand-soft text-brand">
        {icon}
      </span>
      <div className="text-sm font-semibold text-text">{title}</div>
      <p className="mt-1 max-w-xs text-note text-text-muted">{detail}</p>
    </div>
  );
}
