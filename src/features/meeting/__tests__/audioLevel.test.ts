import { describe, expect, it } from "vitest";
import { audioLevel } from "../audioLevel";
describe("recording levels", () => {
  it("shows normal speech levels instead of leaving the meter almost empty", () => {
    expect(audioLevel(0.01)).toBeCloseTo(1 / 3);
    expect(audioLevel(0.03)).toBeGreaterThan(audioLevel(0.01));
    expect(audioLevel(0.1)).toBeCloseTo(2 / 3);
  });
  it("returns to silence and safely clamps invalid or clipped samples", () => {
    for (const rms of [0, -1, 0.0001, Number.NaN, Infinity])
      expect(audioLevel(rms)).toBe(0);
    expect(audioLevel(1)).toBe(1);
    expect(audioLevel(5)).toBe(1);
  });
});
