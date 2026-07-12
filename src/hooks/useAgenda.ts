import { useQueries, useQuery } from "@tanstack/react-query";
import { databasesApi } from "@/lib/api";
import {
  buildAgenda,
  extractTasks,
  pickDueField,
  todayISO,
  type Agenda,
  type AgendaTask,
} from "@/lib/agenda";

/** A database that can hold dated tasks, with the ids quick-add needs. */
export interface PlannerDb {
  id: string;
  pageId: string;
  title: string;
  nameFieldId?: string;
  dueFieldId?: string;
}

export interface AgendaResult {
  agenda: Agenda;
  done: AgendaTask[];
  tasks: AgendaTask[];
  databases: PlannerDb[];
  today: string;
  loading: boolean;
}

/**
 * Aggregate dated tasks across every database (any DB with a date field) into a
 * daily-planner agenda: overdue / today / this-week / later, plus the completed
 * list and per-database metadata for quick-add. All from existing commands, so
 * it works in the browser mock too.
 */
export function useAgenda(): AgendaResult {
  const { data: dbs = [] } = useQuery({
    queryKey: ["databases"],
    queryFn: () => databasesApi.list(),
    staleTime: 10_000,
  });
  const dated = dbs.filter((d) => d.fields.some((f) => f.type === "date"));

  const results = useQueries({
    queries: dated.map((d) => ({
      queryKey: ["database-by-id", d.database_id] as const,
      queryFn: () => databasesApi.getById(d.database_id),
      staleTime: 10_000,
    })),
  });

  const today = todayISO();
  const tasks = dated.flatMap((d, i) => {
    const bundle = results[i]?.data;
    return bundle ? extractTasks(bundle, { pageId: d.page_id, title: d.title }) : [];
  });

  const databases: PlannerDb[] = dated.map((d, i) => {
    const bundle = results[i]?.data;
    const nameField = bundle?.fields.find((f) => f.type === "text");
    const dueF = bundle ? pickDueField(bundle.fields) : undefined;
    return {
      id: d.database_id,
      pageId: d.page_id,
      title: d.title,
      nameFieldId: nameField?.id,
      dueFieldId: dueF?.id,
    };
  });

  return {
    agenda: buildAgenda(tasks, today),
    done: tasks
      .filter((t) => t.done)
      .sort((a, b) => b.due.localeCompare(a.due)),
    tasks,
    databases,
    today,
    loading: results.some((r) => r.isLoading),
  };
}
