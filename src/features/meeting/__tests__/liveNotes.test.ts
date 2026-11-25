import { describe, expect, it } from "vitest";
import { UPDATE_AFTER_CHARS, UPDATE_EVERY_MS, updateDue } from "../LiveNotesPanel";

describe("updateDue", () => {
  it("waits for both time and new speech before updating by itself", () => {
    const start = 1_000_000;
    // The first update comes once enough has been said.
    expect(updateDue(start, 0, UPDATE_AFTER_CHARS - 1, 0)).toBe(false);
    expect(updateDue(start, 0, UPDATE_AFTER_CHARS, 0)).toBe(true);
    // Soon after an update, nothing more happens however much is said.
    expect(updateDue(start + UPDATE_EVERY_MS - 1, start, 5_000, 0)).toBe(false);
    // Later, only if people have kept talking.
    expect(updateDue(start + UPDATE_EVERY_MS, start, 400, 300)).toBe(false);
    expect(updateDue(start + UPDATE_EVERY_MS, start, 600, 300)).toBe(true);
  });
});
