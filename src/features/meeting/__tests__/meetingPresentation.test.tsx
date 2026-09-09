import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MeetingNotes } from "../MeetingNotes";
import { MeetingAnswerText } from "../MeetingAnswerText";

describe("meeting presentation", () => {
  it("renders organised notes and keeps model markup inert", () => {
    const html = renderToStaticMarkup(
      <MeetingNotes
        summary={{
          summary: "",
          sections: [
            { heading: "Handovers", points: ["Jordan updates the notes."] },
          ],
          action_items: ["Priya sends the checklist."],
          decisions: [],
          open_questions: ["<script>alert('no')</script>"],
        }}
      />,
    );
    expect(html).toContain("Handovers</h3>");
    expect(html).toContain("Next steps</h3>");
    expect(html).toContain("Open questions</h3>");
    expect(html).not.toContain("<script>");
  });
  it("links valid claim citations and leaves invalid citations as text", () => {
    const html = renderToStaticMarkup(
      <MeetingAnswerText
        text={"- Handover agreed. [1]\n- Unknown reference. [9]"}
        sourceCount={1}
      />,
    );
    expect(html).toContain('href="#meeting-source-1"');
    expect(html).not.toContain('href="#meeting-source-9"');
    expect(html).toContain("[9]");
  });
});
