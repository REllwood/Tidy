import { describe, expect, it } from "vitest";
import {
  clock,
  firstTime,
  nextStepParts,
  speakerColours,
  talkShare,
  transcriptTurns,
} from "../meetingTimeline";

const body = JSON.stringify([
  { type: "heading", props: { level: 2 }, content: "Meeting notes" },
  { type: "paragraph", content: "Not part of the transcript." },
  { type: "heading", props: { level: 2 }, content: "Transcript" },
  { type: "paragraph", content: "Ana" },
  { type: "paragraph", content: "[00:00:02] Let's start with the beta." },
  { type: "paragraph", content: [{ type: "text", text: "[00:00:08] About forty teams signed up." }] },
  { type: "paragraph", content: "You" },
  { type: "paragraph", content: "[00:00:15] The empty state is the problem." },
  { type: "paragraph", content: "Ana" },
  { type: "paragraph", content: "[00:01:30] Back after a long pause." },
]);

describe("transcriptTurns", () => {
  it("reads who spoke when, joining one person's lines into a turn", () => {
    expect(transcriptTurns(body, 100)).toEqual([
      { speaker: "Ana", start: 2, end: 15 },
      { speaker: "You", start: 15, end: 45 },
      { speaker: "Ana", start: 90, end: 100 },
    ]);
  });

  it("copes with a transcript without speakers or with bad JSON", () => {
    const plain = JSON.stringify([
      { type: "paragraph", content: "[00:05] Hello." },
      { type: "paragraph", content: "[00:09] Again." },
    ]);
    expect(transcriptTurns(plain)).toEqual([{ speaker: null, start: 5, end: 19 }]);
    expect(transcriptTurns("not json")).toEqual([]);
  });
});

describe("talkShare", () => {
  it("gives each speaker's share of the talking, most first", () => {
    expect(talkShare(transcriptTurns(body, 100))).toEqual([
      { speaker: "You", seconds: 30, share: 57 },
      { speaker: "Ana", seconds: 23, share: 43 },
    ]);
    expect(talkShare([{ speaker: null, start: 0, end: 10 }])).toEqual([]);
  });
});

describe("speakerColours", () => {
  it("keeps the brand colour for You and gives others their own", () => {
    const colours = speakerColours(["Ana", "You", "Ben", "Ana"]);
    expect(colours.get("You")).toBe("var(--brand)");
    expect(colours.get("Ana")).toBe("var(--speaker-1)");
    expect(colours.get("Ben")).toBe("var(--speaker-2)");
  });
});

describe("nextStepParts", () => {
  it("takes a next step apart into who, what and by when", () => {
    expect(nextStepParts("Priya will fix the phone number validation by Thursday.")).toEqual({
      owner: "Priya",
      task: "Fix the phone number validation",
      due: "by Thursday",
    });
    expect(nextStepParts("You will book the two training sessions for the week of the 17th.")).toEqual({
      owner: "You",
      task: "Book the two training sessions for the week of the 17th",
      due: null,
    });
    expect(nextStepParts("Send the revised SOW to Acme")).toEqual({
      owner: null,
      task: "Send the revised SOW to Acme",
      due: null,
    });
  });
});

describe("clock and firstTime", () => {
  it("formats short times and finds when a topic starts", () => {
    expect(clock(26)).toBe("0:26");
    expect(clock(3723)).toBe("1:02:03");
    expect(firstTime(["a", "b", "c"], { a: "00:01:10", b: "00:00:52" })).toBe(52);
    expect(firstTime(["a"], undefined)).toBeNull();
  });
});
