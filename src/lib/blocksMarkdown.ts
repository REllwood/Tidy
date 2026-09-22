/**
 * Lightweight, dependency-free BlockNote-JSON ↔ Markdown conversion for our
 * block set. BlockNote JSON stays the source of truth; markdown is for
 * export/import interchange, and round-trips headings, nested lists,
 * checklists, quotes, tables and the inline marks we support.
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Block = Record<string, any>;
type Styles = Record<string, unknown>;

const LIST_TYPES = new Set([
  "bulletListItem",
  "numberedListItem",
  "checkListItem",
]);
const QUOTE_TYPES = new Set(["quote", "callout", "alert"]);
const INDENT = "  ";

// ---- blocks → markdown ----------------------------------------------------

/** Wrap a run in its mark delimiters, keeping surrounding spaces outside. */
function applyStyles(text: string, styles: unknown): string {
  const marks = (styles ?? {}) as Styles;
  const parts = /^(\s*)([\s\S]*?)(\s*)$/.exec(text);
  if (!parts) return text;
  let inner = parts[2];
  if (!inner) return text;
  if (marks.code) inner = `\`${inner}\``;
  if (marks.italic) inner = `_${inner}_`;
  if (marks.bold) inner = `**${inner}**`;
  if (marks.strike) inner = `~~${inner}~~`;
  return `${parts[1]}${inner}${parts[3]}`;
}

function runToText(run: unknown): string {
  if (typeof run === "string") return run;
  if (!run || typeof run !== "object") return "";
  const node = run as Block;
  if (node.type === "link") {
    const label = inlineToText(node.content) || String(node.href ?? "");
    return `[${label}](${node.href ?? ""})`;
  }
  if (node.type === "wikilink") return `[[${node.props?.title ?? ""}]]`;
  if (typeof node.text === "string") return applyStyles(node.text, node.styles);
  if (Array.isArray(node.content)) return inlineToText(node.content);
  return "";
}

export function inlineToText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map(runToText).join("");
  return "";
}

function tableLines(content: unknown, pad: string): string[] {
  const rows = (content as Block)?.rows;
  if (!Array.isArray(rows) || rows.length === 0) return [];
  const cellText = (cell: unknown): string =>
    inlineToText(Array.isArray(cell) ? cell : (cell as Block)?.content)
      .replace(/\|/g, "\\|")
      .replace(/\s+/g, " ")
      .trim();
  const grid = rows.map((row: Block) => (row?.cells ?? []).map(cellText));
  const width = grid.reduce((max, row) => Math.max(max, row.length), 0);
  const line = (cells: string[]) =>
    `${pad}| ${Array.from({ length: width }, (_, i) => cells[i] ?? "").join(" | ")} |`;
  return [
    line(grid[0]),
    `${pad}| ${Array.from({ length: width }, () => "---").join(" | ")} |`,
    ...grid.slice(1).map(line),
  ];
}

function mediaLine(block: Block, text: string, pad: string): string {
  const url = String(block.props?.url ?? "");
  const label =
    text || String(block.props?.caption ?? block.props?.name ?? "") || url;
  return block.type === "image"
    ? `${pad}![${label}](${url})`
    : `${pad}[${label}](${url})`;
}

function renderBlocks(blocks: Block[], depth: number): string[] {
  const pad = INDENT.repeat(depth);
  const lines: string[] = [];
  let counter = 0;
  let previousWasList = false;

  for (const block of blocks ?? []) {
    if (!block || typeof block !== "object") continue;
    const isList = LIST_TYPES.has(block.type);
    if (lines.length && !(isList && previousWasList)) lines.push("");
    previousWasList = isList;
    if (block.type !== "numberedListItem") counter = 0;

    const text = inlineToText(block.content);
    switch (block.type) {
      case "heading":
        lines.push(`${pad}${"#".repeat(block.props?.level ?? 1)} ${text}`);
        break;
      case "bulletListItem":
        lines.push(`${pad}- ${text}`);
        break;
      case "numberedListItem":
        counter += 1;
        lines.push(`${pad}${counter}. ${text}`);
        break;
      case "checkListItem":
        lines.push(`${pad}- [${block.props?.checked ? "x" : " "}] ${text}`);
        break;
      case "codeBlock":
        lines.push(`${pad}\`\`\`${block.props?.language ?? ""}`.trimEnd());
        for (const codeLine of text.split("\n")) lines.push(`${pad}${codeLine}`);
        lines.push(`${pad}\`\`\``);
        break;
      case "divider":
        lines.push(`${pad}---`);
        break;
      case "table":
        lines.push(...tableLines(block.content, pad));
        break;
      case "image":
      case "video":
      case "audio":
      case "file":
        lines.push(mediaLine(block, text, pad));
        break;
      default:
        if (QUOTE_TYPES.has(block.type)) lines.push(`${pad}> ${text}`);
        else lines.push(text ? `${pad}${text}` : "");
    }

    const children = Array.isArray(block.children) ? block.children : [];
    if (children.length) {
      if (QUOTE_TYPES.has(block.type)) {
        const nested = renderBlocks(children, 0);
        lines.push(...nested.map((line) => `${pad}>${line ? ` ${line}` : ""}`));
      } else {
        lines.push(...renderBlocks(children, depth + 1));
      }
    }
  }
  return lines;
}

export function blocksToMarkdown(blocks: Block[]): string {
  const body = renderBlocks(blocks ?? [], 0)
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return `${body}\n`;
}

// ---- markdown → blocks ----------------------------------------------------

const INLINE_RE =
  /!\[([^\]]*)\]\(([^)]*)\)|\[([^\]]*)\]\(([^)]*)\)|`([^`]+)`|~~([\s\S]+?)~~|\*\*([\s\S]+?)\*\*|__([\s\S]+?)__|\*([^*\n]+?)\*|(?<![A-Za-z0-9])_([^_\n]+?)_(?![A-Za-z0-9])/;

function parseInline(source: string, styles: Styles = {}): Block[] {
  const runs: Block[] = [];
  const pushText = (text: string) => {
    if (!text) return;
    const last = runs[runs.length - 1];
    if (
      last &&
      last.type === "text" &&
      JSON.stringify(last.styles ?? {}) === JSON.stringify(styles)
    ) {
      last.text += text;
      return;
    }
    runs.push({ type: "text", text, styles: { ...styles } });
  };

  let rest = source;
  while (rest.length) {
    const match = INLINE_RE.exec(rest);
    if (!match) {
      pushText(rest);
      break;
    }
    pushText(rest.slice(0, match.index));
    if (match[1] !== undefined) {
      // an inline image has no BlockNote equivalent; keep it readable
      pushText(match[0]);
    } else if (match[3] !== undefined) {
      runs.push({
        type: "link",
        href: match[4],
        content: parseInline(match[3], styles),
      });
    } else if (match[5] !== undefined) {
      runs.push(...parseInline(match[5], { ...styles, code: true }));
    } else if (match[6] !== undefined) {
      runs.push(...parseInline(match[6], { ...styles, strike: true }));
    } else if (match[7] !== undefined || match[8] !== undefined) {
      runs.push(
        ...parseInline(match[7] ?? match[8], { ...styles, bold: true }),
      );
    } else {
      runs.push(
        ...parseInline(match[9] ?? match[10], { ...styles, italic: true }),
      );
    }
    rest = rest.slice(match.index + match[0].length);
  }
  return runs;
}

const TABLE_ROW_RE = /^\|(.*)\|$/;
const DIVIDER_ROW_RE = /^\|[\s:|-]+\|$/;

function splitRow(line: string): string[] {
  return (TABLE_ROW_RE.exec(line.trim())?.[1] ?? "")
    .split(/(?<!\\)\|/)
    .map((cell) => cell.replace(/\\\|/g, "|").trim());
}

function parseBlocks(lines: string[]): Block[] {
  const root: Block[] = [];
  const stack: { indent: number; block: Block }[] = [];

  const addListItem = (indent: number, block: Block) => {
    while (stack.length && indent <= stack[stack.length - 1].indent) stack.pop();
    const parent = stack[stack.length - 1];
    if (parent) (parent.block.children ??= []).push(block);
    else root.push(block);
    stack.push({ indent, block });
  };
  const addBlock = (block: Block) => {
    stack.length = 0;
    root.push(block);
  };

  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();
    if (trimmed === "") {
      i += 1;
      continue;
    }
    const indent = line.length - line.trimStart().length;

    if (trimmed.startsWith("```")) {
      const language = trimmed.slice(3).trim();
      const code: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith("```")) {
        const own = lines[i].length - lines[i].trimStart().length;
        code.push(lines[i].slice(Math.min(indent, own)));
        i += 1;
      }
      i += 1;
      addBlock({
        type: "codeBlock",
        ...(language ? { props: { language } } : {}),
        content: code.join("\n"),
      });
      continue;
    }

    if (trimmed.startsWith(">")) {
      const quoted: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith(">")) {
        quoted.push(lines[i].trim().replace(/^>\s?/, ""));
        i += 1;
      }
      const inner = parseBlocks(quoted);
      const [first, ...rest] = inner;
      addBlock({
        type: "quote",
        content: first?.content ?? [],
        ...(rest.length ? { children: rest } : {}),
      });
      continue;
    }

    if (
      TABLE_ROW_RE.test(trimmed) &&
      i + 1 < lines.length &&
      DIVIDER_ROW_RE.test(lines[i + 1].trim())
    ) {
      const rows: Block[] = [
        { cells: splitRow(trimmed).map((cell) => parseInline(cell)) },
      ];
      i += 2;
      while (i < lines.length && TABLE_ROW_RE.test(lines[i].trim())) {
        rows.push({
          cells: splitRow(lines[i]).map((cell) => parseInline(cell)),
        });
        i += 1;
      }
      addBlock({ type: "table", content: { type: "tableContent", rows } });
      continue;
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(trimmed);
    const check = /^[-*]\s+\[([ xX])\]\s+(.*)$/.exec(trimmed);
    const bullet = /^[-*]\s+(.*)$/.exec(trimmed);
    const numbered = /^\d+\.\s+(.*)$/.exec(trimmed);
    const image = /^!\[([^\]]*)\]\(([^)]*)\)$/.exec(trimmed);

    if (heading) {
      addBlock({
        type: "heading",
        props: { level: Math.min(3, heading[1].length) },
        content: parseInline(heading[2]),
      });
    } else if (trimmed === "---" || trimmed === "***") {
      addBlock({ type: "divider" });
    } else if (image) {
      addBlock({
        type: "image",
        props: { url: image[2], caption: image[1] },
      });
    } else if (check) {
      addListItem(indent, {
        type: "checkListItem",
        props: { checked: check[1].toLowerCase() === "x" },
        content: parseInline(check[2]),
      });
    } else if (bullet) {
      addListItem(indent, {
        type: "bulletListItem",
        content: parseInline(bullet[1]),
      });
    } else if (numbered) {
      addListItem(indent, {
        type: "numberedListItem",
        content: parseInline(numbered[1]),
      });
    } else {
      addBlock({ type: "paragraph", content: parseInline(trimmed) });
    }
    i += 1;
  }
  return root;
}

export function markdownToBlocks(md: string): Block[] {
  const blocks = parseBlocks(md.replace(/\r\n/g, "\n").split("\n"));
  return blocks.length ? blocks : [{ type: "paragraph", content: "" }];
}
