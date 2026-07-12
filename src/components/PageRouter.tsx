import { lazy } from "react";
import { usePages } from "@/hooks/usePages";

const DocumentEditor = lazy(() =>
  import("@/components/editor/Editor").then((m) => ({ default: m.DocumentEditor })),
);
const DatabaseView = lazy(() =>
  import("@/components/db/DatabaseView").then((m) => ({ default: m.DatabaseView })),
);

/** Routes a page to the right surface based on its type (doc vs database). */
export function PageRouter({ pageId }: { pageId: string }) {
  const { data: pages } = usePages();
  const page = pages?.find((p) => p.id === pageId);

  if (page?.type === "database") {
    return <DatabaseView pageId={pageId} />;
  }
  return <DocumentEditor pageId={pageId} />;
}
