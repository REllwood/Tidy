import { lazy } from "react";
import { FileQuestion, Loader2 } from "lucide-react";
import { usePages } from "@/hooks/usePages";
import { useUi } from "@/store/ui";
import { Button } from "@/components/ui/button";

const DocumentEditor = lazy(() =>
  import("@/components/editor/Editor").then((m) => ({ default: m.DocumentEditor })),
);
const DatabaseView = lazy(() =>
  import("@/components/db/DatabaseView").then((m) => ({ default: m.DatabaseView })),
);

/** Routes a page to the right surface based on its type (doc vs database). */
export function PageRouter({ pageId }: { pageId: string }) {
  const { data: pages, isFetching } = usePages();
  const setActivePane = useUi((s) => s.setActivePane);
  const page = pages?.find((p) => p.id === pageId);

  // Wait for the list: opening the editor first would load a document for a
  // page that might not be there any more. A page that's missing while the
  // list is refetching (just created, say) is still loading, not gone.
  if (!pages || (!page && isFetching)) {
    return (
      <div
        role="status"
        className="flex items-center gap-2 p-12 text-sm text-text-muted"
      >
        <Loader2 className="size-4 animate-spin" />
        Opening the page…
      </div>
    );
  }

  if (!page) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <div className="max-w-sm">
          <FileQuestion className="mx-auto mb-3 size-8 text-text-faint" />
          <h2 className="text-lg font-semibold">This page no longer exists</h2>
          <p className="mt-1 text-sm text-text-muted">
            It was deleted, or it lived inside a page that was deleted.
          </p>
          <Button
            className="mt-4"
            onClick={() => setActivePane({ kind: "home" })}
          >
            Go to home
          </Button>
        </div>
      </div>
    );
  }

  if (page.type === "database") {
    return <DatabaseView pageId={pageId} />;
  }
  return <DocumentEditor pageId={pageId} />;
}
