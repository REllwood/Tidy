import { afterEach, describe, expect, it, vi } from "vitest";
import { errorMessage, CommandError } from "../errors";
import { invoke } from "../tauri";
import { invoke as nativeInvoke } from "@tauri-apps/api/core";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/lib/mock/backend", () => ({ mockInvoke: vi.fn() }));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});
describe("native command errors", () => {
  it.each(["local_ai_setup", "local_ai_status", "ai_generate"])(
    "decodes structured failures from %s",
    async (command) => {
      vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
      vi.mocked(nativeInvoke).mockRejectedValue({
        kind: "other",
        message: "AI processing is paused while recording a meeting.",
      });
      await expect(invoke(command)).rejects.toMatchObject({
        name: "CommandError",
        kind: "other",
        message: "AI processing is paused while recording a meeting.",
      });
      await expect(invoke(command)).rejects.toBeInstanceOf(Error);
    },
  );
  it("preserves ordinary errors and supplies a readable fallback", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const original = new Error("Connection interrupted. Retry the download.");
    vi.mocked(nativeInvoke).mockRejectedValue(original);
    await expect(invoke("local_ai_download")).rejects.toBe(original);
    expect(errorMessage({ kind: "other" })).toBe(
      "Something went wrong. Please try again.",
    );
    expect(
      String(new CommandError({ message: "Download cancelled" })),
    ).not.toContain("[object Object]");
    expect(errorMessage("Disk is full")).toBe("Disk is full");
  });
});
