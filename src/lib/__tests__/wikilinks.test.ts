import { describe, it, expect } from "vitest";
import { extractLinksAndTags } from "@/lib/wikilinks";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const doc = (content: any) => [{ type: "paragraph", content }];

describe("extractLinksAndTags", () => {
  it("extracts plain-text [[wikilinks]]", () => {
    const { links } = extractLinksAndTags(
      doc("See [[Note B]] and [[Client X]]."),
    );
    expect(links.map((l) => l.dst_title)).toEqual(["Note B", "Client X"]);
    expect(links.every((l) => l.target_page_id == null)).toBe(true);
  });

  it("extracts #tags", () => {
    const { tags } = extractLinksAndTags(
      doc("Follow up #urgent on #client-acme today."),
    );
    expect(tags.sort()).toEqual(["client-acme", "urgent"]);
  });

  it("reads custom wikilink inline nodes with a resolved pageId", () => {
    const { links } = extractLinksAndTags(
      doc([
        { type: "text", text: "ref " },
        { type: "wikilink", props: { pageId: "p1", title: "Apollo" } },
      ]),
    );
    expect(links).toEqual([
      {
        dst_title: "Apollo",
        target_page_id: "p1",
        context: "ref [[Apollo]]",
      },
    ]);
  });

  it("keeps a picked link when an unfinished [[ sits before it", () => {
    const { links } = extractLinksAndTags(
      doc([
        { type: "text", text: "see [[Proj then " },
        { type: "wikilink", props: { pageId: "p1", title: "Projects" } },
      ]),
    );
    expect(links.map((l) => [l.dst_title, l.target_page_id])).toEqual([
      ["Projects", "p1"],
    ]);
  });

  it("dedupes repeated links and handles inline-content arrays", () => {
    const { links } = extractLinksAndTags(
      doc([{ type: "text", text: "[[A]] again [[A]] and [[B]]" }]),
    );
    expect(links.map((l) => l.dst_title)).toEqual(["A", "B"]);
  });

  it("returns empty for plain prose", () => {
    const r = extractLinksAndTags(doc("just some text"));
    expect(r.links).toEqual([]);
    expect(r.tags).toEqual([]);
  });

  it("matches a link split across formatting runs", () => {
    const { links } = extractLinksAndTags(
      doc([
        { type: "text", text: "see [[Pro" },
        { type: "text", text: "ject", styles: { bold: true } },
        { type: "text", text: " Nova]] now" },
      ]),
    );
    expect(links.map((l) => l.dst_title)).toEqual(["Project Nova"]);
  });

  it("reads links and tags inside a link run's nested content", () => {
    const { links, tags } = extractLinksAndTags(
      doc([
        {
          type: "link",
          href: "https://example.test",
          content: [{ type: "text", text: "[[Nested]] #deep" }],
        },
      ]),
    );
    expect(links.map((l) => l.dst_title)).toEqual(["Nested"]);
    expect(tags).toEqual(["deep"]);
  });

  it("recurses into nested child blocks", () => {
    const { links, tags } = extractLinksAndTags([
      {
        type: "bulletListItem",
        content: "top [[One]]",
        children: [
          {
            type: "bulletListItem",
            content: "middle #nested",
            children: [{ type: "bulletListItem", content: "deep [[Two]]" }],
          },
        ],
      },
    ]);
    expect(links.map((l) => l.dst_title)).toEqual(["One", "Two"]);
    expect(tags).toEqual(["nested"]);
  });

  it("reads table cells in both cell shapes", () => {
    const { links } = extractLinksAndTags([
      {
        type: "table",
        content: {
          type: "tableContent",
          rows: [
            { cells: [[{ type: "text", text: "[[Cell A]]" }]] },
            { cells: [{ content: [{ type: "text", text: "[[Cell B]]" }] }] },
          ],
        },
      },
    ]);
    expect(links.map((l) => l.dst_title)).toEqual(["Cell A", "Cell B"]);
  });

  it("keeps a short slice of surrounding text as context", () => {
    const before = "x".repeat(200);
    const after = "y".repeat(200);
    const { links } = extractLinksAndTags(
      doc(`${before} [[Target]] ${after}`),
    );
    const context = links[0].context ?? "";
    expect(context).toContain("[[Target]]");
    expect(context.startsWith("…")).toBe(true);
    expect(context.endsWith("…")).toBe(true);
    expect(context.length).toBeLessThan(120);
  });

  it("gives each link its own context and keeps the first for duplicates", () => {
    const { links } = extractLinksAndTags(
      doc("alpha [[One]] beta. gamma [[Two]] delta."),
    );
    expect(links[0].context).toContain("alpha");
    expect(links[1].context).toContain("gamma");
  });
});
