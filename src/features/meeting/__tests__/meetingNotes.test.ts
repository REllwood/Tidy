import { describe, expect, it } from "vitest";
import { notesOverview } from "../MeetingNotes";

const notes = {
  summary: "The team planned the handover and agreed who owns Acme.",
  action_items: ["Jordan will update the account notes by Thursday."],
  decisions: [],
  sections: [{ heading: "Handover", points: ["Priya takes over Acme."] }],
  open_questions: [],
};

describe("notesOverview", () => {
  it("shows a written overview above the notes", () => {
    expect(notesOverview(notes)).toBe(notes.summary);
  });

  it("hides an overview that only repeats one of the points", () => {
    // Notes from earlier versions used their first point as the overview.
    expect(
      notesOverview({ ...notes, summary: "Priya takes over Acme." }),
    ).toBeNull();
    expect(notesOverview({ ...notes, summary: "  " })).toBeNull();
  });
});
