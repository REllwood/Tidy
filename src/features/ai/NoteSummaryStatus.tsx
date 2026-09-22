import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { noteSummaryApi } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";

/**
 * On a note an agent filed: whether its summary is still coming, and a retry
 * when writing it failed. Shows nothing on every other page.
 */
export function NoteSummaryStatus({ pageId }: { pageId: string }) {
  const qc = useQueryClient();
  const key = ["note-summary-status", pageId];
  const status = useQuery({
    queryKey: key,
    queryFn: () => noteSummaryApi.status(pageId),
    // Only poll while a summary is on its way.
    refetchInterval: (query) => (query.state.data ? 4000 : false),
  });
  const retry = useMutation({
    mutationFn: () => noteSummaryApi.retry(pageId),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });
  const data = status.data;
  if (!data) return null;
  if (data.state === "error")
    return (
      <div
        role="alert"
        className="mb-4 flex flex-wrap items-center gap-3 rounded-lg border border-border bg-surface px-4 py-3 text-sm"
      >
        <p className="min-w-0 flex-1 text-danger-c">
          Tidy couldn't summarise this note. {data.error}
        </p>
        <Button
          size="sm"
          variant="secondary"
          disabled={retry.isPending}
          onClick={() => retry.mutate()}
        >
          {retry.isPending && <Loader2 className="size-3 animate-spin" />}
          Try again
        </Button>
        {retry.error && (
          <p className="w-full text-xs text-danger-c">
            {errorMessage(retry.error)}
          </p>
        )}
      </div>
    );
  return (
    <p
      role="status"
      className="mb-4 flex items-center gap-2 text-sm text-text-muted"
    >
      <Loader2 className="size-3.5 animate-spin" />
      {data.state === "summarising"
        ? "Summarising this note locally…"
        : "Waiting for local AI to summarise this note."}
    </p>
  );
}
