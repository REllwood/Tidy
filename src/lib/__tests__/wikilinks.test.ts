import { describe, it, expect } from "vitest";
import { extractLinksAndTags } from "@/lib/wikilinks";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const doc = (content: any) => [{ type: "paragraph", content }];

describe("extractLinksAndTags", () => {
  it("extracts plain-text [[wikilinks]]", () => {
    const { links } = extractLinksAndTags(doc("See [[Note B]] and [[Client X]]."));
    expect(links.map((l) => l.dst_title)).toEqual(["Note B", "Client X"]);
    expect(links.every((l) => l.target_page_id == null)).toBe(true);
  });

  it("extracts #tags", () => {
    const { tags } = extractLinksAndTags(doc("Follow up #urgent on #client-acme today."));
    expect(tags.sort()).toEqual(["client-acme", "urgent"]);
  });

  it("reads custom wikilink inline nodes with a resolved pageId", () => {
    const { links } = extractLinksAndTags(
      doc([
        { type: "text", text: "ref " },
        { type: "wikilink", props: { pageId: "p1", title: "Apollo" } },
      ]),
    );
    expect(links).toEqual([{ dst_title: "Apollo", target_page_id: "p1" }]);
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
});
