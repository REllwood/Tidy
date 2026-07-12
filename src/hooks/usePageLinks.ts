import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { knowledgeApi } from "@/lib/api";

export function useBacklinks(pageId: string) {
  return useQuery({
    queryKey: ["backlinks", pageId],
    queryFn: () => knowledgeApi.getBacklinks(pageId),
  });
}

export function usePageTags(pageId: string) {
  return useQuery({
    queryKey: ["page-tags", pageId],
    queryFn: () => knowledgeApi.getPageTags(pageId),
  });
}

/** Invalidate backlinks/tags when the core reports a links change (Tauri only). */
export function useLinksEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const un = listen("links-changed", () => {
      qc.invalidateQueries({ queryKey: ["backlinks"] });
      qc.invalidateQueries({ queryKey: ["page-tags"] });
    });
    return () => {
      un.then((f) => f());
    };
  }, [qc]);
}
