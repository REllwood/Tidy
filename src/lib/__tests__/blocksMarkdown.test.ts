import { describe, it, expect } from "vitest";
import { blocksToMarkdown, markdownToBlocks } from "@/lib/blocksMarkdown";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const blocks: any[] = [
  { type: "heading", props: { level: 1 }, content: "Title" },
  { type: "paragraph", content: "Hello world." },
  { type: "bulletListItem", content: "one" },
  { type: "checkListItem", props: { checked: true }, content: "done item" },
  { type: "checkListItem", props: { checked: false }, content: "todo item" },
  { type: "quote", content: "a quote" },
  { type: "codeBlock", content: "let x = 1;" },
];

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
});
