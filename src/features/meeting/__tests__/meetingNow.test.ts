import { describe, expect, it } from "vitest";
import { pickEvent } from "../MeetingNow";
import type { MeetingEvent } from "@/lib/api";

const minute = 60_000;
const event = (title: string, start: number, end: number, people = 0): MeetingEvent => ({
  title,
  start_ms: start * minute,
  end_ms: end * minute,
  attendees: Array.from({ length: people }, (_, i) => `Person ${i}`),
  series: null,
});

describe("pickEvent", () => {
  const events = [
    event("Focus block", 0, 240),
    event("Acme weekly", 60, 90, 3),
    event("Later", 200, 230, 2),
  ];
  it("prefers the meeting with people that is on now", () => {
    expect(pickEvent(events, 70 * minute)).toEqual({ event: events[1], now: true });
    expect(pickEvent(events, 55 * minute)?.event.title).toBe("Acme weekly");
  });
  it("falls back to a personal block, then the next meeting", () => {
    expect(pickEvent(events, 120 * minute)?.event.title).toBe("Focus block");
    expect(pickEvent(events.slice(1), 120 * minute)).toEqual({ event: events[2], now: false });
    expect(pickEvent([], 0)).toBeNull();
  });
});
