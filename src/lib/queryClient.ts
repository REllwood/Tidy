import { QueryClient } from "@tanstack/react-query";

// Local-first app: data comes from the Rust core over Tauri commands. There's no
// network latency, so keep data fresh-by-default and rely on explicit
// invalidation driven by core change-events.
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 0,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});
