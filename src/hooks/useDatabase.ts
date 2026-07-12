import { useEffect } from "react";
import {
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import {
  databasesApi,
  type CellValue,
  type FieldType,
  type ViewConfig,
  type FieldOptions,
  type DatabaseBundle,
} from "@/lib/api";
import { relationTargetIds } from "@/lib/computed";

const key = (pageId: string) => ["database", pageId] as const;

export function useDatabase(pageId: string) {
  return useQuery({
    queryKey: key(pageId),
    queryFn: () => databasesApi.get(pageId),
  });
}

/**
 * Prefetch the target databases referenced by this bundle's relation fields.
 * Returns a map keyed by database id, consumed by `computeCell` to resolve
 * lookups/rollups and by the relation cell editor to render its picker.
 */
export function useRelationTargets(
  bundle: DatabaseBundle | undefined,
): Record<string, DatabaseBundle> {
  const ids = bundle ? relationTargetIds(bundle) : [];
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["database-by-id", id] as const,
      queryFn: () => databasesApi.getById(id),
      staleTime: 10_000,
    })),
  });
  const map: Record<string, DatabaseBundle> = {};
  ids.forEach((id, i) => {
    const data = results[i]?.data;
    if (data) map[id] = data;
  });
  return map;
}

export function useDatabaseEvents(pageId: string) {
  const qc = useQueryClient();
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const un = listen("database-changed", () =>
      qc.invalidateQueries({ queryKey: key(pageId) }),
    );
    return () => {
      un.then((f) => f());
    };
  }, [qc, pageId]);
}

/** Factory: all DB mutations invalidate this page's bundle on settle. */
export function useDbMutations(pageId: string) {
  const qc = useQueryClient();
  const invalidate = () => qc.invalidateQueries({ queryKey: key(pageId) });
  const opts = { onSettled: invalidate };

  return {
    createRow: useMutation({
      mutationFn: (databaseId: string) => databasesApi.createRow(databaseId),
      ...opts,
    }),
    deleteRow: useMutation({
      mutationFn: (id: string) => databasesApi.deleteRow(id),
      ...opts,
    }),
    moveRow: useMutation({
      mutationFn: (v: { id: string; position: number }) =>
        databasesApi.moveRow(v.id, v.position),
      ...opts,
    }),
    setCell: useMutation({
      mutationFn: (v: { rowId: string; fieldId: string; value: CellValue }) =>
        databasesApi.setCell(v.rowId, v.fieldId, v.value),
      // optimistic: patch the cell in the cached bundle for instant feedback
      onMutate: async (v) => {
        await qc.cancelQueries({ queryKey: key(pageId) });
        const prev = qc.getQueryData(key(pageId));
        qc.setQueryData(key(pageId), (old: unknown) => {
          if (!old) return old;
          const b = old as { rows: { id: string; cells: Record<string, CellValue> }[] };
          return {
            ...b,
            rows: b.rows.map((r) =>
              r.id === v.rowId
                ? { ...r, cells: { ...r.cells, [v.fieldId]: v.value } }
                : r,
            ),
          };
        });
        return { prev };
      },
      onError: (_e, _v, ctx) => {
        if (ctx?.prev) qc.setQueryData(key(pageId), ctx.prev);
      },
      onSettled: invalidate,
    }),
    createField: useMutation({
      mutationFn: (v: {
        databaseId: string;
        name: string;
        kind: FieldType;
        options?: FieldOptions;
      }) => databasesApi.createField(v.databaseId, v.name, v.kind, v.options),
      ...opts,
    }),
    deleteField: useMutation({
      mutationFn: (id: string) => databasesApi.deleteField(id),
      ...opts,
    }),
    updateField: useMutation({
      mutationFn: (v: { id: string; name?: string; options?: FieldOptions }) =>
        databasesApi.updateField(v.id, v.name, v.options),
      ...opts,
    }),
    updateView: useMutation({
      mutationFn: (v: { id: string; config: ViewConfig }) =>
        databasesApi.updateView(v.id, v.config),
      ...opts,
    }),
  };
}
