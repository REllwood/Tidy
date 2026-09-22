import { describe, it, expect } from "vitest";
import {
  isReadCommand,
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
  it("drains writes that the savers themselves start", async () => {
    let release!: () => void;
    let rounds = 0;
    const unregister = registerPendingEdits(async () => {
      rounds += 1;
      if (rounds === 1)
        void trackWrite(
          new Promise<void>((r) => {
            release = r;
          }),
        );
    });
    let finished = false;
    const pending = saveBeforeRestart().then(() => {
      finished = true;
    });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(finished).toBe(false);
    release();
    await pending;
    expect(finished).toBe(true);
    expect(rounds).toBeGreaterThanOrEqual(2);
    unregister();
  });
  it("stops after a bounded number of drain rounds", async () => {
    // a saver that always starts another write must not loop forever
    let rounds = 0;
    const unregister = registerPendingEdits(async () => {
      rounds += 1;
      void trackWrite(new Promise<void>((r) => setTimeout(r, 0)));
    });
    try {
      await saveBeforeRestart();
      expect(rounds).toBe(10);
    } finally {
      unregister();
    }
  });
  it("treats reads as untracked and everything else as a write", () => {
    for (const read of [
      "get_document",
      "get_backlinks",
      "list_pages",
      "list_models",
      "search",
      "is_recording",
      "recording_history",
      "transcript_versions",
      "local_ai_status",
      "ollama_status",
      "mcp_get_token",
      "diarization_available",
      "check_app_update",
      "ai_generate",
      "confirm_close",
    ])
      expect(isReadCommand(read)).toBe(true);
    for (const write of [
      "update_document",
      "update_document_if_unchanged",
      "set_page_links",
      "rename_page",
      "set_cell",
      "ingest_note",
      "retranscribe_meeting",
      "flush_page",
      "set_vault_dir",
      "local_ai_configure",
    ])
      expect(isReadCommand(write)).toBe(false);
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
