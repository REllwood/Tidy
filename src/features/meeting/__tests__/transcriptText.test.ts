import { describe, it, expect } from "vitest";
import { transcriptText } from "../transcriptText";
describe("retained transcript text", () => {
  it("keeps timestamps, speaker labels and nested text without executing markup", () => {
    expect(
      transcriptText(
        JSON.stringify([
          { type: "heading", content: "Transcript" },
          {
            content: [{ text: "Speaker 1" }],
            children: [
              {
                content: [
                  { text: "[00:14] Keep <script> literal." },
                  { type: "link", content: [{ text: " Reference" }] },
                ],
              },
            ],
          },
        ]),
      ),
    ).toBe(
      "Transcript\n\nSpeaker 1\n\n[00:14] Keep <script> literal. Reference",
    );
  });
  it("reports malformed stored text instead of returning an empty export", () => {
    expect(() => transcriptText("broken")).toThrow();
    expect(() => transcriptText('{"unexpected":true}')).toThrow();
  });
});
