import { isReadCommand, trackWrite } from "./pendingEdits";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { CommandError } from "./errors";
import { mockInvoke } from "@/lib/mock/backend";

/** True when running inside the Tauri runtime (vs a plain-web dev preview). */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Command bridge. In the native app this is Tauri's `invoke`; in a plain-web
 * preview it routes to an in-memory mock that mirrors the Rust commands, so the
 * whole UI is runnable/verifiable in a browser.
 *
 * Anything that is not a known read is tracked as a write, so closing or
 * updating the app waits for it.
 */
export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    const request = isTauri()
      ? tauriInvoke<T>(cmd, args)
      : mockInvoke<T>(cmd, args ?? {});
    return await (isReadCommand(cmd) ? request : trackWrite(request));
  } catch (error) {
    throw error instanceof Error ? error : new CommandError(error);
  }
}
