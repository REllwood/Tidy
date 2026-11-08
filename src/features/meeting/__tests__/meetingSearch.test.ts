import { describe, expect, it } from "vitest";
import { meetingMatches, searchWords } from "../meetingLibrary";
import type { MeetingJob } from "@/lib/localAi";

const job = (page_id: string, title: string, started_at: number, decisions: string[]): MeetingJob => ({
  page_id,
  title,
  state: "ready",
  error: null,
  summary: { summary: `Notes for ${title}.`, decisions, action_items: [], sections: [] },
  indexed: true,
  client_id: null,
  client_name: null,
  passage_count: 1,
  started_at,
  template: "general",
});

const jobs = [
  job("a", "Weekly product sync", 2_000, ["Launch on the 12th.", "Keep the price at $25."]),
  job("b", "Design review", 3_000, ["Keep the dark theme."]),
  job("c", "Launch planning", 1_000, []),
];

describe("searchWords", () => {
  it("keeps the words worth finding", () => {
    expect(searchWords("When is the launch, and who writes the release notes?")).toEqual([
      "launch",
      "writes",
      "release",
      "notes",
    ]);
    expect(searchWords("What did we decide last week?")).toEqual(["decide"]);
  });
});

describe("meetingMatches", () => {
  it("finds meetings by their notes and titles, newest first", () => {
    const found = meetingMatches(jobs, ["launch"]);
    expect(found.map((m) => m.job.page_id)).toEqual(["a", "c"]);
    expect(found[0].points).toEqual(["Launch on the 12th."]);
    expect(found[1].points).toEqual(["Notes for Launch planning."]);
    // The point matching more of the words comes first.
    const weekly = meetingMatches(jobs, ["price", "keep"]).find((m) => m.job.page_id === "a");
    expect(weekly?.points[0]).toBe("Keep the price at $25.");
  });

  it("lists every meeting with its overview when there is nothing to find", () => {
    const all = meetingMatches(jobs, []);
    expect(all.map((m) => m.job.page_id)).toEqual(["b", "a", "c"]);
    expect(all[0].points).toEqual(["Notes for Design review."]);
  });
});
