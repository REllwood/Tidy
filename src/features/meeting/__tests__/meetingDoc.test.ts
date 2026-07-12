import { describe, it, expect } from "vitest";
import { buildMeetingBlocks } from "../meetingDoc";
import type { TranscriptSegment, MeetingSummary } from "@/lib/api";

const segs: TranscriptSegment[] = [
  { start_ms: 0, end_ms: 4000, text: "Hello team." },
  { start_ms: 65000, end_ms: 68000, text: "Wrap up." },
];
const summary: MeetingSummary = {
  summary: "A short standup.",
  action_items: ["Ship beta", "Write docs"],
  decisions: ["Use Tauri"],
};

describe("buildMeetingBlocks", () => {
  it("composes summary, action items (checkboxes), decisions, transcript when Ollama used", () => {
    const blocks = buildMeetingBlocks(segs, summary, true);
    const types = blocks.map((b) => b.type);
    expect(types).toContain("heading");
    expect(blocks.filter((b) => b.type === "checkListItem")).toHaveLength(2);
    expect(blocks.filter((b) => b.type === "bulletListItem")).toHaveLength(1);
    // transcript timestamps formatted mm:ss
    const transcript = blocks.filter((b) => String(b.content).startsWith("["));
    expect(transcript[0].content).toBe("[00:00] Hello team.");
    expect(transcript[1].content).toBe("[01:05] Wrap up.");
  });

  it("falls back to transcript-only with an install note when Ollama absent", () => {
    const blocks = buildMeetingBlocks(segs, null, false);
    expect(blocks.some((b) => String(b.content).includes("Ollama wasn't running"))).toBe(true);
    expect(blocks.filter((b) => b.type === "checkListItem")).toHaveLength(0);
    expect(blocks.some((b) => b.content === "Transcript")).toBe(true);
  });

  it("handles an empty transcript", () => {
    const blocks = buildMeetingBlocks([], summary, true);
    expect(blocks.some((b) => String(b.content).includes("No speech detected"))).toBe(true);
  });
});
