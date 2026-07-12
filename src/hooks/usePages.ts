import { useEffect } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { pagesApi, buildTree, type Page, type PageType } from "@/lib/api";

const PAGES_KEY = ["pages"] as const;

export function usePages() {
  return useQuery({ queryKey: PAGES_KEY, queryFn: pagesApi.list });
}

export function usePagesTree() {
  const q = usePages();
  return { ...q, tree: q.data ? buildTree(q.data) : [] };
}

/** Subscribe once to core `pages-changed` events → invalidate the cache. */
export function usePagesEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    // Only meaningful inside the Tauri runtime; skip in plain-web previews.
    if (!("__TAURI_INTERNALS__" in window)) return;
    const unlisten = listen("pages-changed", () => {
      qc.invalidateQueries({ queryKey: PAGES_KEY });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [qc]);
}

export function useCreatePage() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      parentId = null,
      title = "Untitled",
      kind = "doc",
    }: {
      parentId?: string | null;
      title?: string;
      kind?: PageType;
    }) => pagesApi.create(parentId, title, kind),
    onSuccess: () => qc.invalidateQueries({ queryKey: PAGES_KEY }),
  });
}

/** Generic optimistic patch for single-page field updates. */
function useOptimisticPatch<TVars extends { id: string }>(
  mutationFn: (v: TVars) => Promise<Page>,
  patch: (p: Page, v: TVars) => Page,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn,
    onMutate: async (vars) => {
      await qc.cancelQueries({ queryKey: PAGES_KEY });
      const prev = qc.getQueryData<Page[]>(PAGES_KEY);
      if (prev) {
        qc.setQueryData<Page[]>(
          PAGES_KEY,
          prev.map((p) => (p.id === vars.id ? patch(p, vars) : p)),
        );
      }
      return { prev };
    },
    onError: (_e, _v, ctx) => {
      if (ctx?.prev) qc.setQueryData(PAGES_KEY, ctx.prev);
    },
    onSettled: () => qc.invalidateQueries({ queryKey: PAGES_KEY }),
  });
}

export function useRenamePage() {
  return useOptimisticPatch<{ id: string; title: string }>(
    ({ id, title }) => pagesApi.rename(id, title),
    (p, { title }) => ({ ...p, title }),
  );
}

export function useSetPageIcon() {
  return useOptimisticPatch<{ id: string; icon: string | null }>(
    ({ id, icon }) => pagesApi.setIcon(id, icon),
    (p, { icon }) => ({ ...p, icon }),
  );
}

export function useSetFavorite() {
  return useOptimisticPatch<{ id: string; isFavorite: boolean }>(
    ({ id, isFavorite }) => pagesApi.setFavorite(id, isFavorite),
    (p, { isFavorite }) => ({ ...p, is_favorite: isFavorite }),
  );
}

export function useMovePage() {
  return useOptimisticPatch<{
    id: string;
    parentId: string | null;
    position: number;
  }>(
    ({ id, parentId, position }) => pagesApi.move(id, parentId, position),
    (p, { parentId, position }) => ({
      ...p,
      parent_id: parentId,
      position,
    }),
  );
}

export function useDeletePage() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => pagesApi.delete(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: PAGES_KEY }),
  });
}
