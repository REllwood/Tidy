import { describe, it, expect } from "vitest";
import { blocksToMarkdown, markdownToBlocks } from "@/lib/blocksMarkdown";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyBlock = any;

const blocks: AnyBlock[] = [
  { type: "heading", props: { level: 1 }, content: "Title" },
  { type: "paragraph", content: "Hello world." },
  { type: "bulletListItem", content: "one" },
  { type: "checkListItem", props: { checked: true }, content: "done item" },
  { type: "checkListItem", props: { checked: false }, content: "todo item" },
  { type: "quote", content: "a quote" },
  { type: "codeBlock", content: "let x = 1;" },
];

const text = (runs: AnyBlock): string =>
  Array.isArray(runs)
    ? runs.map((r: AnyBlock) => r.text ?? "").join("")
    : String(runs ?? "");

describe("blocksMarkdown", () => {
  it("exports the expected markdown", () => {
    const md = blocksToMarkdown(blocks);
    expect(md).toContain("# Title");
    expect(md).toContain("- one");
    expect(md).toContain("- [x] done item");
    expect(md).toContain("- [ ] todo item");
    expect(md).toContain("> a quote");
    expect(md).toContain("```\nlet x = 1;\n```");
  });

  it("round-trips block types through markdown", () => {
    const md = blocksToMarkdown(blocks);
    const parsed = markdownToBlocks(md);
    const types = parsed.map((b) => b.type);
    expect(types).toContain("heading");
    expect(types).toContain("bulletListItem");
    expect(types.filter((t) => t === "checkListItem")).toHaveLength(2);
    expect(types).toContain("quote");
    expect(types).toContain("codeBlock");
    const checks = parsed.filter((b) => b.type === "checkListItem");
    expect(checks[0].props.checked).toBe(true);
    expect(checks[1].props.checked).toBe(false);
  });

  it("handles empty markdown", () => {
    expect(markdownToBlocks("")).toEqual([{ type: "paragraph", content: "" }]);
  });

  it("writes link runs as markdown links", () => {
    const md = blocksToMarkdown([
      {
        type: "paragraph",
        content: [
          { type: "text", text: "See ", styles: {} },
          {
            type: "link",
            href: "https://example.test/docs",
            content: [{ type: "text", text: "the docs", styles: {} }],
          },
          { type: "text", text: " today.", styles: {} },
        ],
      },
    ]);
    expect(md.trim()).toBe("See [the docs](https://example.test/docs) today.");
  });

  it("round-trips links back into link runs", () => {
    const parsed = markdownToBlocks(
      "See [the docs](https://example.test/docs) today.",
    );
    const runs = parsed[0].content as AnyBlock[];
    expect(runs[1]).toMatchObject({
      type: "link",
      href: "https://example.test/docs",
    });
    expect(text(runs[1].content)).toBe("the docs");
  });

  it("round-trips inline marks", () => {
    const original: AnyBlock[] = [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "bold", styles: { bold: true } },
          { type: "text", text: " and ", styles: {} },
          { type: "text", text: "italic", styles: { italic: true } },
          { type: "text", text: " and ", styles: {} },
          { type: "text", text: "code", styles: { code: true } },
          { type: "text", text: " and ", styles: {} },
          { type: "text", text: "gone", styles: { strike: true } },
        ],
      },
    ];
    const md = blocksToMarkdown(original);
    expect(md.trim()).toBe("**bold** and _italic_ and `code` and ~~gone~~");
    const runs = markdownToBlocks(md)[0].content as AnyBlock[];
    expect(runs.map((r) => [r.text, r.styles])).toEqual([
      ["bold", { bold: true }],
      [" and ", {}],
      ["italic", { italic: true }],
      [" and ", {}],
      ["code", { code: true }],
      [" and ", {}],
      ["gone", { strike: true }],
    ]);
  });

  it("leaves snake_case words alone", () => {
    const runs = markdownToBlocks("call some_helper_name now")[0]
      .content as AnyBlock[];
    expect(text(runs)).toBe("call some_helper_name now");
    expect(runs.every((r) => !r.styles?.italic)).toBe(true);
  });

  it("indents nested list children and reads them back", () => {
    const nested: AnyBlock[] = [
      {
        type: "bulletListItem",
        content: "one",
        children: [
          {
            type: "bulletListItem",
            content: "two",
            children: [{ type: "numberedListItem", content: "deep" }],
          },
        ],
      },
      { type: "bulletListItem", content: "three" },
    ];
    const md = blocksToMarkdown(nested);
    expect(md).toBe("- one\n  - two\n    1. deep\n- three\n");
    const parsed = markdownToBlocks(md);
    expect(parsed).toHaveLength(2);
    expect(parsed[0].children[0].type).toBe("bulletListItem");
    expect(text(parsed[0].children[0].content)).toBe("two");
    expect(parsed[0].children[0].children[0].type).toBe("numberedListItem");
    expect(text(parsed[0].children[0].children[0].content)).toBe("deep");
    expect(text(parsed[1].content)).toBe("three");
  });

  it("numbers ordered lists in sequence", () => {
    const md = blocksToMarkdown([
      { type: "numberedListItem", content: "first" },
      { type: "numberedListItem", content: "second" },
      { type: "numberedListItem", content: "third" },
    ]);
    expect(md).toBe("1. first\n2. second\n3. third\n");
  });

  it("serialises and reads a table", () => {
    const table: AnyBlock[] = [
      {
        type: "table",
        content: {
          type: "tableContent",
          rows: [
            { cells: [["Name"], ["Owner"]] },
            { cells: [["Beta"], ["Ada"]] },
          ],
        },
      },
    ];
    const md = blocksToMarkdown(table);
    expect(md).toBe("| Name | Owner |\n| --- | --- |\n| Beta | Ada |\n");
    const parsed = markdownToBlocks(md);
    expect(parsed[0].type).toBe("table");
    expect(parsed[0].content.rows).toHaveLength(2);
    expect(text(parsed[0].content.rows[1].cells[1])).toBe("Ada");
  });

  it("writes image and file blocks as markdown media", () => {
    const md = blocksToMarkdown([
      { type: "image", props: { url: "https://x.test/a.png", caption: "Chart" } },
      { type: "file", props: { url: "https://x.test/b.pdf", name: "Report" } },
    ]);
    expect(md).toContain("![Chart](https://x.test/a.png)");
    expect(md).toContain("[Report](https://x.test/b.pdf)");
  });

  it("keeps quote children inside the quote", () => {
    const md = blocksToMarkdown([
      {
        type: "quote",
        content: "Top line",
        children: [{ type: "paragraph", content: "More detail" }],
      },
    ]);
    expect(md).toBe("> Top line\n> More detail\n");
    const parsed = markdownToBlocks(md);
    expect(parsed[0].type).toBe("quote");
    expect(text(parsed[0].content)).toBe("Top line");
    expect(text(parsed[0].children[0].content)).toBe("More detail");
  });

  it("keeps the fenced language on a code block", () => {
    const md = blocksToMarkdown([
      { type: "codeBlock", props: { language: "ts" }, content: "const a = 1;" },
    ]);
    expect(md).toBe("```ts\nconst a = 1;\n```\n");
    const parsed = markdownToBlocks(md);
    expect(parsed[0]).toMatchObject({
      type: "codeBlock",
      props: { language: "ts" },
      content: "const a = 1;",
    });
  });
});
