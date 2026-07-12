import { pagesApi, documentsApi } from "@/lib/api";
import { blocksToMarkdown, markdownToBlocks } from "@/lib/blocksMarkdown";

function downloadText(filename: string, text: string, mime = "text/markdown") {
  const blob = new Blob([text], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

function safeName(title: string): string {
  return (title.trim() || "Untitled").replace(/[^\w\- ]+/g, "").slice(0, 80);
}

/** Export a page's content as a downloaded .md file. */
export async function exportPageMarkdown(pageId: string): Promise<void> {
  const page = await pagesApi.get(pageId);
  const raw = await documentsApi.get(pageId);
  let blocks: unknown[] = [];
  try {
    blocks = JSON.parse(raw);
  } catch {
    blocks = [];
  }
  const md = `# ${page.title || "Untitled"}\n\n${blocksToMarkdown(blocks as never)}`;
  downloadText(`${safeName(page.title)}.md`, md);
}

/** Print the current view to PDF via the OS print dialog (Save as PDF). */
export function exportPagePdf(): void {
  window.print();
}

/**
 * Import a markdown file as a new page. Returns the new page id.
 * `parentId` nests it; null = top level.
 */
export async function importMarkdownText(
  markdown: string,
  fallbackTitle: string,
  parentId: string | null = null,
): Promise<string> {
  // Use a leading H1 as the page title if present.
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  let title = fallbackTitle;
  let body = markdown;
  const firstNonEmpty = lines.findIndex((l) => l.trim() !== "");
  if (firstNonEmpty >= 0) {
    const h1 = lines[firstNonEmpty].match(/^#\s+(.*)$/);
    if (h1) {
      title = h1[1].trim() || fallbackTitle;
      body = lines.slice(firstNonEmpty + 1).join("\n");
    }
  }
  const page = await pagesApi.create(parentId, title, "doc");
  const blocks = markdownToBlocks(body);
  await documentsApi.update(page.id, JSON.stringify(blocks));
  return page.id;
}

/** Open a file picker and import the chosen .md file; resolves to the new page id. */
export function pickAndImportMarkdown(): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".md,.markdown,text/markdown,text/plain";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      const text = await file.text();
      const fallback = file.name.replace(/\.(md|markdown|txt)$/i, "");
      resolve(await importMarkdownText(text, fallback));
    };
    input.click();
  });
}
