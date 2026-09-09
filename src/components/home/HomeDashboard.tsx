import { useMemo } from "react";
import {
  ArrowRight,
  FilePlus2,
  FileText,
  Mic,
  Star,
  Table2,
  X,
  Loader2,
  Plus,
} from "lucide-react";
import { usePages, useCreatePage } from "@/hooks/usePages";
import { useAgenda } from "@/hooks/useAgenda";
import { useUi } from "@/store/ui";
import { greeting, type AgendaTask } from "@/lib/agenda";
import { TaskRow } from "@/components/planner/TaskRow";
import { Button } from "@/components/ui/button";
import type { Page } from "@/lib/api";
import { MeetingMemory } from "./MeetingMemory";

export function HomeDashboard() {
  const openPage = useUi((s) => s.openPage);
  const setPane = useUi((s) => s.setActivePane);
  const setIngestOpen = useUi((s) => s.setIngestOpen);
  const onboarded = useUi((s) => s.onboarded);
  const setOnboarded = useUi((s) => s.setOnboarded);
  const create = useCreatePage();
  const {
    data: pages = [],
    isPending: pagesPending,
    error: pagesError,
  } = usePages();
  const { agenda, today, loading } = useAgenda();
  const recent = useMemo(
    () => [...pages].sort((a, b) => b.updated_at - a.updated_at).slice(0, 5),
    [pages],
  );
  const favourites = useMemo(() => pages.filter((p) => p.is_favorite), [pages]);
  const now = new Date();
  const count =
    agenda.overdue.length + agenda.today.length + agenda.week.length;
  const createPage = () =>
    create.mutate({ title: "Untitled" }, { onSuccess: (p) => openPage(p.id) });
  return (
    <div className="h-full overflow-y-auto">
      <div className="home-page mx-auto max-w-6xl px-6 py-9 sm:px-10 sm:py-12">
        <div className="mb-9 flex flex-wrap items-end justify-between gap-6">
          <div>
            <p className="page-eyebrow">
              {now.toLocaleDateString("en-AU", {
                weekday: "long",
                day: "numeric",
                month: "long",
              })}
            </p>
            <h1 className="mt-3 font-editorial text-[clamp(36px,4.5vw,52px)] font-normal leading-tight tracking-[-0.04em]">
              {greeting(now).replace(/[.!]$/, "")}.
            </h1>
            <p className="mt-2 text-sm text-text-muted">
              A little space to think. A clear place to begin.
            </p>
          </div>
          <Button
            onClick={() => setPane({ kind: "meeting" })}
            className="h-10 gap-2 px-4"
          >
            <Mic className="size-4" />
            New meeting
          </Button>
        </div>
        {!onboarded && (
          <div className="mb-7 flex items-start gap-4 rounded-xl border border-border bg-surface px-5 py-4">
            <div className="flex-1">
              <h2 className="text-sm font-semibold">Make this space yours</h2>
              <p className="mt-1 text-note text-text-muted">
                Start with a note, plan your week, or record a conversation.
                Your work stays on your Mac.
              </p>
              <button
                disabled={create.isPending}
                onClick={createPage}
                className="mt-3 flex items-center gap-2 text-note font-medium text-brand"
              >
                {create.isPending ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <Plus className="size-3.5" />
                )}
                Write your first page
              </button>
            </div>
            <button
              onClick={() => setOnboarded(true)}
              aria-label="Dismiss welcome"
              className="rounded p-1 text-text-faint hover:text-text"
            >
              <X className="size-4" />
            </button>
          </div>
        )}
        <div className="mb-9 flex flex-wrap gap-2 border-y border-border py-3">
          <QuickAction
            icon={<FileText className="size-4" />}
            label="File a note"
            onClick={() => setIngestOpen(true)}
          />
          <QuickAction
            icon={<FilePlus2 className="size-4" />}
            label={create.isPending ? "Creating page…" : "New page"}
            busy={create.isPending}
            onClick={createPage}
          />
          <QuickAction
            icon={<Table2 className="size-4" />}
            label="Open planner"
            onClick={() => setPane({ kind: "planner" })}
          />
          <span className="ml-auto hidden self-center text-xs text-text-faint sm:block">
            Everything in its place.
          </span>
        </div>
        {create.error && (
          <p role="alert" className="mb-5 text-sm text-danger-c">
            Couldn't create the page. {String(create.error)}
          </p>
        )}
        <div className="home-layout">
          <div className="min-w-0">
            <section aria-label="Your agenda">
              <div className="mb-5 flex items-baseline justify-between gap-3">
                <h2 className="text-lg font-semibold tracking-tight">
                  Coming into focus
                </h2>
                <button
                  onClick={() => setPane({ kind: "planner" })}
                  className="flex items-center gap-1.5 text-xs font-medium text-brand"
                >
                  View planner
                  <ArrowRight className="size-3.5" />
                </button>
              </div>
              <div className="mb-6 grid grid-cols-3 gap-3 border-b border-border pb-5">
                {[
                  { label: "Due today", value: agenda.today.length },
                  { label: "This week", value: agenda.week.length },
                  { label: "Overdue", value: agenda.overdue.length },
                ].map((item) => (
                  <div key={item.label}>
                    <p
                      className={`text-3xl font-light tabular-nums ${item.label === "Overdue" && item.value ? "text-danger-c" : "text-text"}`}
                    >
                      {loading ? "—" : item.value}
                    </p>
                    <p className="mt-1 text-xs text-text-muted">{item.label}</p>
                  </div>
                ))}
              </div>
              {loading ? (
                <p
                  role="status"
                  className="flex items-center gap-2 py-6 text-sm text-text-muted"
                >
                  <Loader2 className="size-4 animate-spin" />
                  Gathering your tasks…
                </p>
              ) : count === 0 ? (
                <div className="rounded-xl border border-dashed border-border-strong px-5 py-8">
                  <p className="font-editorial text-2xl tracking-tight">
                    Room for what's next.
                  </p>
                  <p className="mt-2 max-w-sm text-note text-text-muted">
                    Add a date to a task in your tables and it will appear here.
                    Your planner keeps the rest in view.
                  </p>
                  <button
                    onClick={() => setPane({ kind: "planner" })}
                    className="mt-4 flex items-center gap-2 text-note font-medium text-brand"
                  >
                    Go to planner
                    <ArrowRight className="size-3.5" />
                  </button>
                </div>
              ) : (
                <div className="space-y-5">
                  <TaskGroup
                    label="Overdue"
                    tasks={agenda.overdue}
                    today={today}
                    overdue
                  />
                  <TaskGroup label="Today" tasks={agenda.today} today={today} />
                  <TaskGroup
                    label="This week"
                    tasks={agenda.week}
                    today={today}
                  />
                </div>
              )}
            </section>
            <section className="mt-9" aria-label="Recently edited pages">
              <h2 className="mb-3 text-sm font-semibold">
                Pick up where you left off
              </h2>
              {pagesPending ? (
                <p
                  role="status"
                  className="flex items-center gap-2 text-xs text-text-muted"
                >
                  <Loader2 className="size-3 animate-spin" />
                  Loading pages…
                </p>
              ) : pagesError ? (
                <p role="alert" className="text-xs text-danger-c">
                  Couldn't load your pages.
                </p>
              ) : recent.length ? (
                <div className="divide-y divide-border">
                  {recent.map((page) => (
                    <PageRow
                      key={page.id}
                      page={page}
                      onOpen={() => openPage(page.id)}
                    />
                  ))}
                </div>
              ) : (
                <p className="py-3 text-sm text-text-muted">
                  Your recently edited pages will appear here.
                </p>
              )}
            </section>
          </div>
          <div className="min-w-0 space-y-7">
            <MeetingMemory />
            <section aria-label="Favourite pages">
              <div className="mb-3 flex items-center gap-2">
                <Star className="size-3.5 text-text-faint" />
                <h2 className="text-sm font-semibold">Close to hand</h2>
              </div>
              {favourites.length ? (
                favourites.map((page) => (
                  <PageRow
                    key={page.id}
                    page={page}
                    onOpen={() => openPage(page.id)}
                  />
                ))
              ) : (
                <p className="text-note leading-relaxed text-text-muted">
                  Star a page to keep it here. The things you return to, one
                  click away.
                </p>
              )}
            </section>
          </div>
        </div>
      </div>
    </div>
  );
}
function QuickAction({
  icon,
  label,
  onClick,
  busy = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  busy?: boolean;
}) {
  return (
    <button
      disabled={busy}
      onClick={onClick}
      className="flex items-center gap-2 rounded-lg px-3 py-2 text-note text-text-muted hover:bg-surface-hover hover:text-text disabled:opacity-60"
    >
      {busy ? <Loader2 className="size-4 animate-spin" /> : icon}
      {label}
    </button>
  );
}
function TaskGroup({
  label,
  tasks,
  today,
  overdue = false,
}: {
  label: string;
  tasks: AgendaTask[];
  today: string;
  overdue?: boolean;
}) {
  if (!tasks.length) return null;
  return (
    <div>
      <div className="mb-2 flex items-center gap-2">
        <h3
          className={`text-xs font-medium ${overdue ? "text-danger-c" : "text-text-muted"}`}
        >
          {label}
        </h3>
        <span className="text-2xs text-text-faint">{tasks.length}</span>
      </div>
      <div className="workspace-panel divide-y divide-border overflow-hidden">
        {tasks.slice(0, 5).map((task) => (
          <TaskRow
            key={task.rowId}
            task={task}
            today={today}
            overdue={overdue}
          />
        ))}
      </div>
      {tasks.length > 5 && (
        <p className="mt-2 text-xs text-text-faint">
          {tasks.length - 5} more in Planner
        </p>
      )}
    </div>
  );
}
function PageRow({ page, onOpen }: { page: Page; onOpen: () => void }) {
  return (
    <button
      onClick={onOpen}
      className="group flex w-full items-center gap-3 rounded-lg px-1 py-3 text-left transition-colors hover:bg-surface-hover"
    >
      <span className="grid size-8 shrink-0 place-items-center rounded-lg border border-border bg-surface text-sm">
        {page.icon ??
          (page.type === "database" ? (
            <Table2 className="size-3.5 text-text-faint" />
          ) : (
            <FileText className="size-3.5 text-text-faint" />
          ))}
      </span>
      <span className="min-w-0 flex-1 truncate text-note font-medium">
        {page.title || "Untitled"}
      </span>
      <ArrowRight className="size-3.5 shrink-0 text-text-faint opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
    </button>
  );
}
