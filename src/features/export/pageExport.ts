import {
  pagesApi,
  documentsApi,
  databasesApi,
  type DatabaseBundle,
} from "@/lib/api";
import { flushPendingEdits } from "@/lib/pendingEdits";
import { blocksToMarkdown, markdownToBlocks } from "@/lib/blocksMarkdown";
import { invoke, isTauri } from "@/lib/tauri";

/** Some WebViews never fire `cancel`; wait this long after focus returns. */
const PICKER_CANCEL_GRACE_MS = 500;

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

/** A file-system-safe name that keeps accents and non-Latin scripts. */
export function safeName(title: string): string {
  const cleaned = title
    .replace(/[^\p{L}\p{N}\-_ ]+/gu, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 80)
    .trim();
  return cleaned || "Untitled";
}

/** A table's rows as a Markdown table, with the fields a reader can use. */
export function databaseMarkdown(bundle: DatabaseBundle): string {
  const shown = bundle.fields
    .filter((f) => ["text", "number", "select", "date", "checkbox"].includes(f.type))
    .sort((a, b) => a.position - b.position);
  if (!shown.length || !bundle.rows.length) return "";
  const cell = (value: unknown) =>
    String(value ?? "")
      .replace(/\|/g, "\\|")
      .replace(/\r?\n/g, " ")
      .trim();
  const text = (row: DatabaseBundle["rows"][number]) =>
    shown.map((f) => {
      const value = row.cells[f.id];
      if (f.type === "checkbox") return value ? "Yes" : "";
      if (f.type === "select")
        return cell(f.options?.choices?.find((c) => c.id === value)?.name);
      return cell(value);
    });
  const rows = [...bundle.rows].sort((a, b) => a.position - b.position);
  return [
    `| ${shown.map((f) => cell(f.name) || " ").join(" | ")} |`,
    `| ${shown.map(() => "---").join(" | ")} |`,
    ...rows.map((row) => `| ${text(row).join(" | ")} |`),
  ].join("\n");
}

/**
 * Save exported text. The desktop webview cancels browser downloads, so the
 * app writes the file through the system save panel. Resolves to false when
 * the panel was closed without saving.
 */
async function saveText(filename: string, text: string): Promise<boolean> {
  if (!isTauri()) {
    downloadText(filename, text);
    return true;
  }
  return invoke<boolean>("save_text_file", {
    fileName: filename,
    contents: text,
    extension: "md",
  });
}

/** Export a page as a Markdown file. Resolves to false if the save was cancelled. */
export async function exportPageMarkdown(pageId: string): Promise<boolean> {
  // the open editor may still be holding the newest edit
  await flushPendingEdits();
  const page = await pagesApi.get(pageId);
  let body = "";
  if (page.type === "database") {
    body = databaseMarkdown(await databasesApi.get(pageId));
  } else {
    const raw = await documentsApi.get(pageId);
    let blocks: unknown[] = [];
    try {
      blocks = JSON.parse(raw);
    } catch {
      blocks = [];
    }
    body = blocksToMarkdown(blocks as never);
  }
  const md = `# ${page.title || "Untitled"}\n\n${body}`;
  return saveText(`${safeName(page.title)}.md`, md);
}

/**
 * Print the current view to PDF via the OS print dialog (Save as PDF). The
 * desktop app routes window.print() through the webview, which can refuse.
 */
export async function exportPagePdf(): Promise<void> {
  await Promise.resolve(window.print() as unknown);
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

/**
 * Open a file picker and import the chosen .md file; resolves to the new page
 * id, or null when the picker was dismissed. Always settles, so the caller's
 * "Importing note…" state cannot stick.
 */
export function pickAndImportMarkdown(): Promise<string | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".md,.markdown,text/markdown,text/plain";
    // Firefox and some WebViews only fire events for an attached input.
    input.style.position = "fixed";
    input.style.left = "-9999px";
    input.style.opacity = "0";

    let settled = false;
    let grace: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (grace) clearTimeout(grace);
      grace = null;
      window.removeEventListener("focus", onFocus);
      input.removeEventListener("change", onChange);
      input.removeEventListener("cancel", onCancel);
      input.remove();
    };
    const finish = (value: string | null) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(value);
    };
    const fail = (error: unknown) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };

    const onChange = async () => {
      if (grace) clearTimeout(grace);
      grace = null;
      const file = input.files?.[0];
      if (!file) return finish(null);
      try {
        const text = await file.text();
        const fallback = file.name.replace(/\.(md|markdown|txt)$/i, "");
        finish(await importMarkdownText(text, fallback));
      } catch (error) {
        fail(error);
      }
    };
    const onCancel = () => finish(null);
    const onFocus = () => {
      if (grace) clearTimeout(grace);
      grace = setTimeout(() => {
        if (!input.files?.length) finish(null);
      }, PICKER_CANCEL_GRACE_MS);
    };

    input.addEventListener("change", onChange);
    input.addEventListener("cancel", onCancel);
    window.addEventListener("focus", onFocus);
    document.body.appendChild(input);
    input.click();
  });
}
