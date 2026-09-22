import { useEffect, useMemo, useRef } from "react";
import { toast } from "sonner";
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
import { errorMessage } from "@/lib/errors";

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
 *
 * The map identity is stable while the underlying data is unchanged, so it can
 * safely sit in the dependency list of the views' memos.
 */
export function useRelationTargets(
  bundle: DatabaseBundle | undefined,
): Record<string, DatabaseBundle> {
  const ids = useMemo(() => (bundle ? relationTargetIds(bundle) : []), [bundle]);
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: ["database-by-id", id] as const,
      queryFn: () => databasesApi.getById(id),
      staleTime: 10_000,
    })),
  });

  // `useQueries` hands back a fresh array every render, so the map is rebuilt
  // only when one of the queries actually produced new data.
  const signature = ids
    .map((id, i) => `${id}:${results[i]?.dataUpdatedAt ?? 0}`)
    .join("|");
  const cached = useRef<Record<string, DatabaseBundle>>({});
  const lastSignature = useRef<string | null>(null);
  if (lastSignature.current !== signature) {
    lastSignature.current = signature;
    const map: Record<string, DatabaseBundle> = {};
    ids.forEach((id, i) => {
      const data = results[i]?.data;
      if (data) map[id] = data;
    });
    cached.current = map;
  }
  return cached.current;
}

/**
 * Listen for native database changes. Mounted once by the app shell: every
 * database query is invalidated, because a change can touch a page bundle, a
 * relation target or the database list.
 */
export function useDatabaseEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const un = listen("database-changed", () => {
      qc.invalidateQueries({ queryKey: ["database"] });
      qc.invalidateQueries({ queryKey: ["database-by-id"] });
      qc.invalidateQueries({ queryKey: ["databases"] });
    });
    return () => {
      un.then((f) => f());
    };
  }, [qc]);
}

/**
 * Factory: every DB mutation invalidates the queries all surfaces read (the
 * per-page bundle, the by-id bundles the planner and relations use, and the
 * database list for schema changes), and reports failures with the native
 * error's own message.
 */
export function useDbMutations(pageId: string) {
  const qc = useQueryClient();
  const invalidate = () =>
    Promise.all([
      qc.invalidateQueries({ queryKey: ["database"] }),
      qc.invalidateQueries({ queryKey: ["database-by-id"] }),
    ]);
  const invalidateSchema = () =>
    Promise.all([invalidate(), qc.invalidateQueries({ queryKey: ["databases"] })]);

  const fail = (what: string) => (error: unknown) =>
    toast.error(what, { description: errorMessage(error) });

  const rowOpts = (what: string) => ({
    onSettled: invalidate,
    onError: fail(what),
  });
  const fieldOpts = (what: string) => ({
    onSettled: invalidateSchema,
    onError: fail(what),
  });

  return {
    createRow: useMutation({
      mutationFn: (databaseId: string) => databasesApi.createRow(databaseId),
      ...rowOpts("Couldn't add the row"),
    }),
    deleteRow: useMutation({
      mutationFn: (id: string) => databasesApi.deleteRow(id),
      ...rowOpts("Couldn't delete the row"),
    }),
    moveRow: useMutation({
      mutationFn: (v: { id: string; position: number }) =>
        databasesApi.moveRow(v.id, v.position),
      ...rowOpts("Couldn't move the row"),
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
          const b = old as {
            rows: { id: string; cells: Record<string, CellValue> }[];
          };
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
      onError: (error, _v, ctx) => {
        if (ctx?.prev) qc.setQueryData(key(pageId), ctx.prev);
        fail("Couldn't save that change")(error);
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
      ...fieldOpts("Couldn't add the field"),
    }),
    deleteField: useMutation({
      mutationFn: (id: string) => databasesApi.deleteField(id),
      ...fieldOpts("Couldn't delete the field"),
    }),
    updateField: useMutation({
      mutationFn: (v: { id: string; name?: string; options?: FieldOptions }) =>
        databasesApi.updateField(v.id, v.name, v.options),
      ...fieldOpts("Couldn't update the field"),
    }),
    updateView: useMutation({
      mutationFn: (v: { id: string; config: ViewConfig }) =>
        databasesApi.updateView(v.id, v.config),
      ...rowOpts("Couldn't save the view"),
    }),
  };
}
