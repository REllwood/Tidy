/**
 * Lightweight, dependency-free BlockNote-JSON ↔ Markdown conversion for our
 * block set. Lossy (styles/nesting flatten), BlockNote JSON stays the source
 * of truth; markdown is for export/import interchange.
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Block = Record<string, any>;

function inlineToText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .map((run: any) => (typeof run === "string" ? run : (run?.text ?? "")))
      .join("");
  }
  return "";
}

export function blocksToMarkdown(blocks: Block[]): string {
  const lines: string[] = [];
  for (const b of blocks) {
    const text = inlineToText(b.content);
    switch (b.type) {
      case "heading":
        lines.push(`${"#".repeat(b.props?.level ?? 1)} ${text}`);
        break;
      case "bulletListItem":
        lines.push(`- ${text}`);
        break;
      case "numberedListItem":
        lines.push(`1. ${text}`);
        break;
      case "checkListItem":
        lines.push(`- [${b.props?.checked ? "x" : " "}] ${text}`);
        break;
      case "codeBlock":
        lines.push("```\n" + text + "\n```");
        break;
      case "quote":
        lines.push(`> ${text}`);
        break;
      case "divider":
        lines.push("---");
        break;
      default:
        lines.push(text);
    }
  }
  return lines.join("\n\n").trim() + "\n";
}

export function markdownToBlocks(md: string): Block[] {
  const blocks: Block[] = [];
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();
    if (trimmed === "") {
      i++;
      continue;
    }
    // fenced code block
    if (trimmed.startsWith("```")) {
      const code: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) {
        code.push(lines[i]);
        i++;
      }
      i++; // skip closing fence
      blocks.push({ type: "codeBlock", content: code.join("\n") });
      continue;
    }
    const heading = trimmed.match(/^(#{1,6})\s+(.*)$/);
    const check = trimmed.match(/^[-*]\s+\[([ xX])\]\s+(.*)$/);
    const bullet = trimmed.match(/^[-*]\s+(.*)$/);
    const numbered = trimmed.match(/^\d+\.\s+(.*)$/);
    const quote = trimmed.match(/^>\s+(.*)$/);
    if (heading) {
      blocks.push({
        type: "heading",
        props: { level: Math.min(3, heading[1].length) },
        content: heading[2],
      });
    } else if (trimmed === "---" || trimmed === "***") {
      blocks.push({ type: "divider" });
    } else if (check) {
      blocks.push({
        type: "checkListItem",
        props: { checked: check[1].toLowerCase() === "x" },
        content: check[2],
      });
    } else if (bullet) {
      blocks.push({ type: "bulletListItem", content: bullet[1] });
    } else if (numbered) {
      blocks.push({ type: "numberedListItem", content: numbered[1] });
    } else if (quote) {
      blocks.push({ type: "quote", content: quote[1] });
    } else {
      blocks.push({ type: "paragraph", content: trimmed });
    }
    i++;
  }
  return blocks.length ? blocks : [{ type: "paragraph", content: "" }];
}
