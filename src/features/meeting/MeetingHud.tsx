import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CircleDot, Timer } from "lucide-react";
import { meetingHudApi, type MeetingHud as Hud } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { countdownCause } from "./recorderPresentation";

/**
 * The small window that floats above a call: it offers to record when a call
 * starts, and counts down before Tidy stops a recording that has finished.
 * It never takes focus from the meeting app.
 */
export function MeetingHud() {
  const [hud, setHud] = useState<Hud>({ kind: "none" });
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    void meetingHudApi.state().then(setHud).catch(() => undefined);
    const unlisten = listen<Hud>("meeting-hud", (e) => setHud(e.payload));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (hud.kind !== "countdown") return;
    const timer = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(timer);
  }, [hud.kind]);

  const respond = (action: "record" | "dismiss" | "keep") =>
    void meetingHudApi.respond(action).catch(() => undefined);

  return (
    <div
      data-tauri-drag-region
      className="flex h-screen flex-col justify-end gap-2.5 bg-surface px-4 pb-3.5 pt-8 text-text select-none"
    >
      {hud.kind === "offer" && (
        <>
          <div data-tauri-drag-region className="flex items-start gap-2.5">
            <CircleDot className="mt-0.5 size-4 shrink-0 text-rec-text" />
            <div data-tauri-drag-region>
              <p className="text-sm font-semibold">
                Record this {hud.app} call?
              </p>
              <p className="text-xs text-text-muted">
                Tell everyone on the call before you record.
              </p>
            </div>
          </div>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => respond("dismiss")}>
              Not now
            </Button>
            <Button size="sm" onClick={() => respond("record")}>
              Record
            </Button>
          </div>
        </>
      )}
      {hud.kind === "countdown" && (
        <>
          <div data-tauri-drag-region className="flex items-start gap-2.5">
            <Timer className="mt-0.5 size-4 shrink-0 text-warning" />
            <div data-tauri-drag-region>
              <p className="text-sm font-semibold">
                Stopping in{" "}
                {Math.max(0, Math.ceil((hud.deadline_ms - now) / 1000))} s
              </p>
              <p className="text-xs text-text-muted">
                {countdownCause(hud.reason)} The transcript is saved when it
                stops.
              </p>
            </div>
          </div>
          <div className="flex justify-end">
            <Button size="sm" onClick={() => respond("keep")}>
              Keep recording
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
