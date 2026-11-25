import { describe, expect, it } from "vitest";
import { UPDATE_AFTER_CHARS, UPDATE_EVERY_MS, updateDue } from "../useLiveNotes";

describe("updateDue", () => {
  it("waits for both time and new speech before updating by itself", () => {
    const every = UPDATE_EVERY_MS.local;
    const start = 1_000_000;
    // The first update comes once enough has been said.
    expect(updateDue(start, 0, UPDATE_AFTER_CHARS - 1, 0, every)).toBe(false);
    expect(updateDue(start, 0, UPDATE_AFTER_CHARS, 0, every)).toBe(true);
    // Soon after an update, nothing more happens however much is said.
    expect(updateDue(start + every - 1, start, 5_000, 0, every)).toBe(false);
    // Later, only if people have kept talking.
    expect(updateDue(start + every, start, 400, 300, every)).toBe(false);
    expect(updateDue(start + every, start, 600, 300, every)).toBe(true);
  });

  it("asks Claude and Codex less often than this Mac", () => {
    expect(UPDATE_EVERY_MS.claude).toBeGreaterThan(UPDATE_EVERY_MS.local);
    expect(UPDATE_EVERY_MS.codex).toBe(UPDATE_EVERY_MS.claude);
    const start = 1_000_000;
    expect(updateDue(start + UPDATE_EVERY_MS.local, start, 600, 0, UPDATE_EVERY_MS.codex)).toBe(false);
  });
});
