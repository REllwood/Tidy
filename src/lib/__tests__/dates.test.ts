import { describe, it, expect } from "vitest";
import { parseLocalDate, dayNum, addDays, dayKey, toIsoDay } from "@/lib/dates";

/** A Date stand-in with a fixed epoch and zone offset, so the bucketing of a
 *  local calendar day can be checked without depending on the host timezone. */
const at = (utcMs: number, offsetMinutes: number) =>
  ({
    getTime: () => utcMs,
    getTimezoneOffset: () => offsetMinutes,
  }) as unknown as Date;

const utcDay = (y: number, m: number, d: number) =>
  Math.floor(Date.UTC(y, m, d) / 86_400_000);

describe("parseLocalDate", () => {
  it("parses YYYY-MM-DD in local time (no UTC off-by-one)", () => {
    const d = parseLocalDate("2026-06-20")!;
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(5);
    expect(d.getDate()).toBe(20);
  });
  it("keeps the local day for a full ISO timestamp", () => {
    expect(parseLocalDate("2026-06-20T23:30:00")!.getDate()).toBe(20);
  });
  it("passes a Date through and rejects empty/invalid input", () => {
    const d = new Date(2026, 0, 2);
    expect(parseLocalDate(d)).toBe(d);
    expect(parseLocalDate("")).toBeNull();
    expect(parseLocalDate(null)).toBeNull();
    expect(parseLocalDate("not a date")).toBeNull();
  });
});

describe("dayNum", () => {
  it("buckets by LOCAL calendar day east of UTC", () => {
    // Sydney (UTC+11): local midnight on 20 June is 19 June 13:00 UTC.
    expect(dayNum(at(Date.UTC(2026, 5, 19, 13, 0), -660))).toBe(
      utcDay(2026, 5, 20),
    );
  });
  it("buckets by LOCAL calendar day west of UTC", () => {
    // Los Angeles (UTC-7): local midnight on 20 June is 20 June 07:00 UTC.
    expect(dayNum(at(Date.UTC(2026, 5, 20, 7, 0), 420))).toBe(
      utcDay(2026, 5, 20),
    );
  });
  it("gives one bucket per local day, whatever the time of day", () => {
    const morning = new Date(2026, 5, 20, 0, 1);
    const night = new Date(2026, 5, 20, 23, 59);
    expect(dayNum(morning)).toBe(dayNum(night));
    expect(dayNum(new Date(2026, 5, 21)) - dayNum(morning)).toBe(1);
  });
  it("matches the calendar day the date string names", () => {
    expect(dayNum(parseLocalDate("2026-06-20")!)).toBe(utcDay(2026, 5, 20));
  });
});

describe("addDays / dayKey / toIsoDay", () => {
  it("steps whole local days across a month boundary", () => {
    expect(toIsoDay(addDays(new Date(2026, 5, 29), 3))).toBe("2026-07-02");
    expect(toIsoDay(addDays(new Date(2026, 5, 1), -1))).toBe("2026-05-31");
  });
  it("keys raw cell values, or null when they aren't dates", () => {
    expect(dayKey("2026-06-20")).toBe(utcDay(2026, 5, 20));
    expect(dayKey("")).toBeNull();
    expect(dayKey(undefined)).toBeNull();
  });
});
