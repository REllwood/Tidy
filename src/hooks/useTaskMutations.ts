import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { databasesApi, type CellValue, type DatabaseBundle } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import type { AgendaTask } from "@/lib/agenda";

interface SetCellVars {
  databaseId: string;
  rowId: string;
  fieldId: string;
  value: CellValue;
}

const bundleKey = (databaseId: string) =>
  ["database-by-id", databaseId] as const;

/**
 * Act on a planner task in place: toggle completion (flip the Status select) or
 * reschedule (set the due date). The change lands in the one cached bundle the
 * agenda reads from straight away, then the affected queries refetch.
 */
export function useTaskMutations() {
  const qc = useQueryClient();

  const setCell = useMutation({
    mutationFn: (v: SetCellVars) =>
      databasesApi.setCell(v.rowId, v.fieldId, v.value),
    onMutate: async (v) => {
      const key = bundleKey(v.databaseId);
      await qc.cancelQueries({ queryKey: key });
      const prev = qc.getQueryData<DatabaseBundle>(key);
      if (prev) {
        qc.setQueryData<DatabaseBundle>(key, {
          ...prev,
          rows: prev.rows.map((r) =>
            r.id === v.rowId
              ? { ...r, cells: { ...r.cells, [v.fieldId]: v.value } }
              : r,
          ),
        });
      }
      return { prev };
    },
    onError: (error, v, ctx) => {
      if (ctx?.prev) qc.setQueryData(bundleKey(v.databaseId), ctx.prev);
      toast.error(`Couldn't update the task. ${errorMessage(error)}`);
    },
    // Only the bundle that changed, plus the per-page database views.
    onSettled: (_data, _error, v) =>
      Promise.all([
        qc.invalidateQueries({ queryKey: bundleKey(v.databaseId) }),
        qc.invalidateQueries({ queryKey: ["database"] }),
      ]),
  });

  const canComplete = (t: AgendaTask) => !!t.statusFieldId && !!t.doneChoiceId;

  const complete = (t: AgendaTask) => {
    if (!t.statusFieldId || !t.doneChoiceId) return;
    const value = t.done ? t.todoChoiceId : t.doneChoiceId;
    setCell.mutate({
      databaseId: t.databaseId,
      rowId: t.rowId,
      fieldId: t.statusFieldId,
      value,
    });
  };

  const reschedule = (t: AgendaTask, iso: string) => {
    setCell.mutate({
      databaseId: t.databaseId,
      rowId: t.rowId,
      fieldId: t.dueFieldId,
      value: iso,
    });
  };

  return { complete, reschedule, canComplete, pending: setCell.isPending };
}
