import { describe, it, expect } from "vitest";
import { assignSpeakers } from "../meetingDoc";
import type { TranscriptSegment, SpeakerSegment } from "@/lib/api";

const transcript: TranscriptSegment[] = [
  { start_ms: 0, end_ms: 4000, text: "a" },
  { start_ms: 4000, end_ms: 9000, text: "b" },
  { start_ms: 9000, end_ms: 14000, text: "c" },
];
const speakers: SpeakerSegment[] = [
  { start_ms: 0, end_ms: 8000, speaker: 0 },
  { start_ms: 8000, end_ms: 15000, speaker: 1 },
];

describe("assignSpeakers", () => {
  it("assigns the max-overlap speaker to each segment", () => {
    const out = assignSpeakers(transcript, speakers);
    expect(out.map((s) => s.speaker)).toEqual([0, 0, 1]);
  });

  it("leaves speaker undefined when there is no overlap", () => {
    const out = assignSpeakers(
      [{ start_ms: 20000, end_ms: 21000, text: "x" }],
      speakers,
    );
    expect(out[0].speaker).toBeUndefined();
  });

  it("handles empty speaker list", () => {
    const out = assignSpeakers(transcript, []);
    expect(out.every((s) => s.speaker === undefined)).toBe(true);
  });
});
