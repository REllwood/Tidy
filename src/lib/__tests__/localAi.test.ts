import { describe, it, expect } from "vitest";
import { dateRange } from "../localAi";

describe("meeting date filters", () => {
  it("uses an inclusive end date and preserves unbounded searches", () => {
    expect(dateRange("", "")).toEqual({ from: null, until: null });
    const range = dateRange("2026-09-09", "2026-09-09");
    expect(range.from).toBe(new Date("2026-09-09T00:00:00").getTime());
    expect(range.until).toBe(new Date("2026-09-10T00:00:00").getTime());
  });
  it("rejects reversed or invalid ranges", () => {
    expect(() => dateRange("2026-09-10", "2026-09-08")).toThrow();
    expect(() => dateRange("invalid", "")).toThrow();
  });
});
