import { useQuery } from "@tanstack/react-query";
import { documentsApi } from "@/lib/api";

export function useDocument(pageId: string) {
  return useQuery({
    queryKey: ["document", pageId],
    queryFn: () => documentsApi.get(pageId),
    // editor owns the doc after load; don't refetch underneath it
    staleTime: Infinity,
    gcTime: 0,
  });
}
