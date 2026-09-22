import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import { invoke, isTauri } from "@/lib/tauri";
import { flushPendingEdits, saveBeforeRestart } from "@/lib/pendingEdits";

/** Only show the overlay when saving takes long enough to be noticed. */
const OVERLAY_DELAY_MS = 400;

/**
 * Holds the window open while unsaved editor work is written.
 *
 * Inside Tauri the core emits `close-requested` and waits for `confirm_close`
 * (with its own fallback), so the confirmation is always sent, even when a
 * save fails. In a browser preview there is nothing to hold, so we flush on
 * the last events the page reliably gets.
 */
export function CloseGuard() {
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const flush = () => {
      void flushPendingEdits();
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") flush();
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", flush);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("pagehide", flush);
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let overlay: ReturnType<typeof setTimeout> | null = null;

    const listening = listen("close-requested", async () => {
      overlay = setTimeout(() => {
        if (!disposed) setSaving(true);
      }, OVERLAY_DELAY_MS);
      try {
        await saveBeforeRestart();
      } catch (error) {
        console.error("Could not save everything before closing", error);
      } finally {
        if (overlay) clearTimeout(overlay);
        overlay = null;
        if (!disposed) setSaving(false);
        try {
          await invoke("confirm_close");
        } catch (error) {
          console.error("Could not confirm the close", error);
        }
      }
    });

    return () => {
      disposed = true;
      if (overlay) clearTimeout(overlay);
      void listening
        .then((unlisten) => unlisten())
        .catch((error) => console.error("Close listener cleanup", error));
    };
  }, []);

  if (!saving) return null;
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/70 backdrop-blur-sm">
      <p
        role="status"
        className="flex items-center gap-2 rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text shadow-md"
      >
        <Loader2 className="size-4 animate-spin" />
        Saving your changes…
      </p>
    </div>
  );
}
