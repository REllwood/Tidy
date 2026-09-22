import { useEffect, useMemo } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { pagesApi, buildTree, type Page, type PageType } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { useUi } from "@/store/ui";

const PAGES_KEY = ["pages"] as const;

export function usePages() {
  return useQuery({ queryKey: PAGES_KEY, queryFn: pagesApi.list });
}

/**
 * Put a page the app just created into the cached list before opening it.
 * The shell sends a pane whose page isn't in the list back Home, and the
 * refetch can land after the page is opened.
 */
export function rememberPage(qc: QueryClient, page: Page) {
  qc.setQueryData<Page[]>(PAGES_KEY, (pages) =>
    pages && !pages.some((p) => p.id === page.id) ? [...pages, page] : pages,
  );
  void qc.invalidateQueries({ queryKey: PAGES_KEY });
}

export function usePagesTree() {
  const q = usePages();
  // Memoised on the query data so node identities stay stable between renders
  // and the memoised tree rows don't all re-render.
  const data = q.data;
  const tree = useMemo(() => (data ? buildTree(data) : []), [data]);
  return { ...q, tree };
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
    onSuccess: (page) => rememberPage(qc, page),
  });
}

/**
 * Generic optimistic patch for single-page field updates.
 *
 * `toastOnError` is off where the surface that runs the mutation reports the
 * failure itself (the page header names the field that wouldn't save), so a
 * failed change never raises two toasts.
 */
function useOptimisticPatch<TVars extends { id: string }>(
  mutationFn: (v: TVars) => Promise<Page>,
  patch: (p: Page, v: TVars) => Page,
  { toastOnError = true }: { toastOnError?: boolean } = {},
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
    onError: (error, _v, ctx) => {
      if (ctx?.prev) qc.setQueryData(PAGES_KEY, ctx.prev);
      if (toastOnError)
        toast.error(`Couldn't save that change. ${errorMessage(error)}`);
    },
    onSettled: () => qc.invalidateQueries({ queryKey: PAGES_KEY }),
  });
}

export function useRenamePage() {
  // The page header reports rename failures itself.
  return useOptimisticPatch<{ id: string; title: string }>(
    ({ id, title }) => pagesApi.rename(id, title),
    (p, { title }) => ({ ...p, title }),
    { toastOnError: false },
  );
}

export function useSetPageIcon() {
  // The page header reports icon failures itself.
  return useOptimisticPatch<{ id: string; icon: string | null }>(
    ({ id, icon }) => pagesApi.setIcon(id, icon),
    (p, { icon }) => ({ ...p, icon }),
    { toastOnError: false },
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
    onSuccess: async (_data, deletedId) => {
      await qc.invalidateQueries({ queryKey: PAGES_KEY });
      // The open page (or an ancestor of it) may have just gone: don't leave
      // the pane pointing at a page that no longer exists.
      const { activePane, setActivePane } = useUi.getState();
      if (activePane.kind !== "page") return;
      const pages = qc.getQueryData<Page[]>(PAGES_KEY);
      const gone =
        activePane.pageId === deletedId ||
        (!!pages && !pages.some((p) => p.id === activePane.pageId));
      if (gone) setActivePane({ kind: "home" });
    },
    onError: (error) =>
      toast.error(`Couldn't delete the page. ${errorMessage(error)}`),
  });
}
