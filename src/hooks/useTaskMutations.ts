import { useMutation, useQueryClient } from "@tanstack/react-query";
import { databasesApi, type CellValue } from "@/lib/api";
import type { AgendaTask } from "@/lib/agenda";

/**
 * Act on a planner task in place: toggle completion (flip the Status select) or
 * reschedule (set the due date). Invalidates the queries the agenda + DB views
 * read so every surface updates.
 */
export function useTaskMutations() {
  const qc = useQueryClient();
  const invalidate = () => {
    // Prefix keys so every dated-DB query the agenda observes refetches, plus the
    // per-page DB view. (The planner reads ["database-by-id", <id>] via useQueries.)
    qc.invalidateQueries({ queryKey: ["database-by-id"] });
    qc.invalidateQueries({ queryKey: ["database"] });
  };
  const setCell = useMutation({
    mutationFn: (v: { rowId: string; fieldId: string; value: CellValue }) =>
      databasesApi.setCell(v.rowId, v.fieldId, v.value),
  });

  const canComplete = (t: AgendaTask) => !!t.statusFieldId && !!t.doneChoiceId;

  const complete = (t: AgendaTask) => {
    if (!t.statusFieldId || !t.doneChoiceId) return;
    const value = t.done ? t.todoChoiceId : t.doneChoiceId;
    setCell.mutate(
      { rowId: t.rowId, fieldId: t.statusFieldId, value },
      { onSuccess: () => invalidate() },
    );
  };

  const reschedule = (t: AgendaTask, iso: string) => {
    setCell.mutate(
      { rowId: t.rowId, fieldId: t.dueFieldId, value: iso },
      { onSuccess: () => invalidate() },
    );
  };

  return { complete, reschedule, canComplete, pending: setCell.isPending };
}
