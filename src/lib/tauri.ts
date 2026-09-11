import { trackWrite } from "./pendingEdits";
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
 */
export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    const request = isTauri() ? tauriInvoke<T>(cmd, args) : mockInvoke<T>(cmd, args ?? {});
    const writes = ["update_document", "update_document_if_unchanged", "rename_page", "set_cell", "update_field", "update_view", "create_page", "set_page_links"];
    return await (writes.includes(cmd) ? trackWrite(request) : request);
  } catch (error) {
    throw error instanceof Error ? error : new CommandError(error);
  }
}
