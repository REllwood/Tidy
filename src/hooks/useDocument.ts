import { useQuery, type QueryClient } from "@tanstack/react-query";
import { documentsApi } from "@/lib/api";

export const documentKey = (pageId: string) => ["document", pageId];

export function useDocument(pageId: string) {
  return useQuery({
    queryKey: documentKey(pageId),
    queryFn: () => documentsApi.get(pageId),
    // editor owns the doc after load; don't refetch underneath it
    staleTime: Infinity,
    gcTime: 0,
  });
}

/**
 * Drop the cached document so the next mount reloads it. Used when the core
 * reports that a page changed outside the editor.
 */
/** Tells an open editor its page was rewritten by the app itself. */
export const DOCUMENT_CHANGED_EVENT = "tidy:document-changed";

/**
 * Refresh a page the app has just rewritten (named speakers, added notes).
 * The desktop core also emits `document-changed`; this covers the moment
 * before it arrives and the browser preview, where it never does.
 */
export function announceDocumentChange(qc: QueryClient, pageId: string) {
  void invalidateDocument(qc, pageId);
  window.dispatchEvent(
    new CustomEvent(DOCUMENT_CHANGED_EVENT, { detail: { page_id: pageId } }),
  );
}

export function invalidateDocument(qc: QueryClient, pageId: string) {
  return qc.invalidateQueries({ queryKey: documentKey(pageId) });
}
