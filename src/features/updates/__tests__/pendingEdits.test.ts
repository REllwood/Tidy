import { describe, it, expect } from "vitest";
import {
  registerPendingEdits,
  saveBeforeRestart,
  trackWrite,
} from "@/lib/pendingEdits";
// @ts-expect-error The release helper is a Node script without a declaration file.
import { updateManifest } from "../../../../scripts/write-update-manifest.mjs";
describe("update preparation", () => {
  it("waits for unsaved editor work and pending writes before restart", async () => {
    let resolve!: () => void;
    let finished = false;
    const write = trackWrite(
      new Promise<void>((r) => {
        resolve = r;
      }),
    );
    const unregister = registerPendingEdits(() => write);
    const pending = saveBeforeRestart().then(() => {
      finished = true;
    });
    await Promise.resolve();
    expect(finished).toBe(false);
    resolve();
    await pending;
    expect(finished).toBe(true);
    unregister();
  });
  it("blocks restart when saving fails", async () => {
    const unregister = registerPendingEdits(async () => {
      throw new Error("Disk full");
    });
    try {
      await expect(saveBeforeRestart()).rejects.toThrow("Disk full");
    } finally {
      unregister();
    }
  });
  it("builds a versioned Apple Silicon feed and rejects invalid versions", () => {
    const manifest = updateManifest(
      "0.2.2",
      "c2lnbmF0dXJl",
      "Fixes recording history.",
    );
    expect(manifest.platforms["darwin-aarch64"].url).toBe(
      "https://github.com/REllwood/Tidy/releases/download/v0.2.2/Tidy.app.tar.gz",
    );
    expect(() => updateManifest("../bad", "c2ln")).toThrow();
    expect(() => updateManifest("0.2.2", "")).toThrow();
  });
});
