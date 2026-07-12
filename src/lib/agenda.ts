import type { DatabaseBundle } from "@/lib/api";

/** A dated, actionable row surfaced from any database, the daily-planner unit. */
export interface AgendaTask {
  rowId: string;
  databaseId: string;
  databasePageId: string;
  databaseTitle: string;
  title: string;
  due: string; // YYYY-MM-DD (local)
  status: string | null;
  statusColor: string | null;
  done: boolean;
  // ids needed to mutate the row from the planner:
  dueFieldId: string;
  statusFieldId: string | null;
  doneChoiceId: string | null;
  todoChoiceId: string | null;
}

export type Bucket = "overdue" | "today" | "week" | "later";
export type Agenda = Record<Bucket, AgendaTask[]>;

/** Local YYYY-MM-DD for a date (no UTC drift). */
export function toISODate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function todayISO(now: Date = new Date()): string {
  return toISODate(now);
}

export function greeting(now: Date = new Date()): string {
  const h = now.getHours();
  if (h < 5) return "Good evening";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

const firstBy = (fields: DatabaseBundle["fields"], type: string) =>
  fields.find((f) => f.type === type);

/** Pick the field that best represents a "due" date. */
export function pickDueField(fields: DatabaseBundle["fields"]) {
  const dates = fields.filter((f) => f.type === "date");
  return (
    dates.find((f) => /due|deadline/i.test(f.name)) ??
    dates.find((f) => /start|date|when/i.test(f.name)) ??
    dates[0]
  );
}

const DONE_RE = /done|complete|shipped|closed/i;

/** Pick the select field that represents task status (not e.g. Priority). */
export function pickStatusField(fields: DatabaseBundle["fields"]) {
  const selects = fields.filter((f) => f.type === "select");
  return (
    selects.find((f) => /status|state|stage/i.test(f.name)) ??
    selects.find((f) => (f.options?.choices ?? []).some((c) => DONE_RE.test(c.name))) ??
    selects[0]
  );
}

/** Extract the dated tasks from one database bundle. */
export function extractTasks(
  bundle: DatabaseBundle,
  meta: { pageId: string; title: string },
): AgendaTask[] {
  const titleField = firstBy(bundle.fields, "text");
  const due = pickDueField(bundle.fields);
  const status = pickStatusField(bundle.fields);
  if (!due) return [];
  const choices = status?.options?.choices ?? [];
  const doneChoiceId = choices.find((c) => DONE_RE.test(c.name))?.id ?? null;
  const todoChoiceId =
    choices.find((c) => /to ?do|backlog|not started|open/i.test(c.name))?.id ??
    // fall back to any non-done choice (never the done choice, that makes
    // "un-complete" a no-op)
    choices.find((c) => c.id !== doneChoiceId)?.id ??
    null;
  return bundle.rows.flatMap((r) => {
    const d = r.cells[due.id];
    if (!d || typeof d !== "string") return [];
    const statusVal = status ? r.cells[status.id] : null;
    const choice = choices.find((c) => c.id === statusVal);
    const done = choice ? DONE_RE.test(choice.name) : false;
    return [
      {
        rowId: r.id,
        databaseId: bundle.database_id,
        databasePageId: meta.pageId,
        databaseTitle: meta.title,
        title: titleField ? String(r.cells[titleField.id] ?? "Untitled") : "Untitled",
        due: d,
        status: choice?.name ?? null,
        statusColor: choice?.color ?? null,
        done,
        dueFieldId: due.id,
        statusFieldId: status?.id ?? null,
        doneChoiceId,
        todoChoiceId,
      },
    ];
  });
}

export function bucketOf(due: string, today: string): Bucket {
  if (due < today) return "overdue";
  if (due === today) return "today";
  // Anchor at noon + round so a DST transition (a 23/25-hour day) can't tip the
  // exactly-7-days boundary between "week" and "later".
  const dt = new Date(`${due}T12:00:00`).getTime();
  const td = new Date(`${today}T12:00:00`).getTime();
  const days = Math.round((dt - td) / 86_400_000);
  return days <= 7 ? "week" : "later";
}

/** Group open (not-done) tasks into overdue / today / this-week / later. */
export function buildAgenda(tasks: AgendaTask[], today: string): Agenda {
  const groups: Agenda = { overdue: [], today: [], week: [], later: [] };
  for (const t of tasks) {
    if (t.done) continue;
    groups[bucketOf(t.due, today)].push(t);
  }
  for (const k of Object.keys(groups) as Bucket[]) {
    groups[k].sort((a, b) => a.due.localeCompare(b.due) || a.title.localeCompare(b.title));
  }
  return groups;
}

export type Snooze = "today" | "tomorrow" | "weekend" | "nextweek";

/** Resolve a snooze target to a local ISO date. */
export function snoozeDate(kind: Snooze, now: Date = new Date()): string {
  const d = new Date(now);
  d.setHours(12, 0, 0, 0); // midday, so DST can't shift the calendar day
  const day = d.getDay(); // 0 Sun … 6 Sat
  if (kind === "tomorrow") d.setDate(d.getDate() + 1);
  else if (kind === "weekend") d.setDate(d.getDate() + ((6 - day + 7) % 7)); // upcoming Saturday
  else if (kind === "nextweek") d.setDate(d.getDate() + (((1 - day + 7) % 7) || 7)); // next Monday
  return toISODate(d);
}

/** A short, human due label relative to today. */
export function dueLabel(due: string, today: string): string {
  if (due === today) return "Today";
  const dt = new Date(`${due}T00:00:00`);
  const td = new Date(`${today}T00:00:00`);
  const days = Math.round((dt.getTime() - td.getTime()) / 86_400_000);
  if (days === 1) return "Tomorrow";
  if (days === -1) return "Yesterday";
  if (days < 0) return `${-days} days ago`;
  if (days < 7)
    return dt.toLocaleDateString(undefined, { weekday: "long" });
  return dt.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
