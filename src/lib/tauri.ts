import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { mockInvoke } from "@/lib/mock/backend";

/** True when running inside the Tauri runtime (vs a plain-web dev preview). */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Command bridge. In the native app this is Tauri's `invoke`; in a plain-web
 * preview it routes to an in-memory mock that mirrors the Rust commands, so the
 * whole UI is runnable/verifiable in a browser.
 */
export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) return tauriInvoke<T>(cmd, args);
  return mockInvoke<T>(cmd, args ?? {});
}
