import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { invoke, isTauri } from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
import { saveBeforeRestart } from "@/lib/pendingEdits";
import { recordingHistoryApi } from "@/lib/api";
import { recordingHistoryKey } from "@/lib/api";
import { useSharedMeetingFlow } from "@/features/meeting/MeetingFlowProvider";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { version } from "../../../package.json";

type Available = { version: string; notes: string | null };
type Phase =
  | "idle"
  | "checking"
  | "ready"
  | "current"
  | "preparing"
  | "downloading"
  | "installing"
  | "restarting"
  | "error";
type Progress = {
  stage: "downloading" | "installing" | "restarting";
  downloaded: number;
  total: number | null;
};
type UpdateControls = {
  phase: Phase;
  message: string;
  available: Available | null;
  check: () => Promise<void>;
  show: () => void;
};
const Context = createContext<UpdateControls | null>(null);
export function useUpdates() {
  const value = useContext(Context);
  if (!value) throw new Error("Update controls are unavailable.");
  return value;
}
export function UpdateProvider({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [message, setMessage] = useState("");
  const [available, setAvailable] = useState<Available | null>(null);
  const [open, setOpen] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const operation = useRef(false);
  const dismissed = useRef<string | null>(null);
  const { state: meeting } = useSharedMeetingFlow();
  const recordingBusy = !["idle", "done", "error"].includes(meeting.phase);
  const busyRef = useRef(recordingBusy);
  busyRef.current = recordingBusy;
  const history = useQuery({
    queryKey: recordingHistoryKey,
    queryFn: recordingHistoryApi.list,
    enabled: open,
    refetchInterval: open ? 3000 : false,
  });
  const blocked =
    recordingBusy ||
    history.data?.some((m) => m.transcript_state === "transcribing");
  const installing = [
    "preparing",
    "downloading",
    "installing",
    "restarting",
  ].includes(phase);
  useEffect(() => {
    if (
      available &&
      !recordingBusy &&
      !installing &&
      dismissed.current !== available.version
    )
      setOpen(true);
  }, [available, recordingBusy, installing]);
  const check = useCallback(async (automatic = false) => {
    if (operation.current || (automatic && busyRef.current)) return;
    operation.current = true;
    setPhase("checking");
    setMessage("");
    try {
      const next = await invoke<Available | null>("check_app_update");
      setAvailable(next);
      setPhase(next ? "ready" : "current");
      setMessage(
        next
          ? `Tidy ${next.version} is available.`
          : `Tidy ${version} is up to date.`,
      );
      if (
        next &&
        !busyRef.current &&
        (!automatic || dismissed.current !== next.version)
      )
        setOpen(true);
    } catch (error) {
      setPhase("error");
      setMessage(errorMessage(error));
    } finally {
      operation.current = false;
    }
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    const initial = setTimeout(() => {
      void check(true);
    }, 15000);
    const interval = setInterval(
      () => {
        void check(true);
      },
      6 * 60 * 60 * 1000,
    );
    return () => {
      clearTimeout(initial);
      clearInterval(interval);
    };
  }, [check]);
  const install = async () => {
    if (!available || operation.current || blocked) return;
    operation.current = true;
    setPhase("preparing");
    setMessage("");
    setProgress(null);
    let unlisten: (() => void) | undefined;
    try {
      await saveBeforeRestart();
      if (isTauri()) {
        unlisten = await listen<Progress>("app-update-progress", (e) => {
          setProgress(e.payload);
          setPhase(e.payload.stage);
        });
        setPhase("downloading");
        await invoke("install_app_update", { version: available.version });
      } else {
        setPhase("downloading");
        await invoke("install_app_update", { version: available.version });
        setPhase("current");
        setAvailable(null);
        setMessage("Preview complete. No update was installed.");
      }
    } catch (error) {
      setPhase("error");
      setMessage(errorMessage(error));
    } finally {
      unlisten?.();
      operation.current = false;
    }
  };
  const label =
    phase === "preparing"
      ? "Saving changes…"
      : phase === "downloading"
        ? `Downloading update${progress?.total ? `… ${Math.min(100, Math.round((progress.downloaded / progress.total) * 100))}%` : "…"}`
        : phase === "installing"
          ? "Installing update…"
          : "Restarting Tidy…";
  return (
    <Context.Provider
      value={{
        phase,
        message,
        available,
        check: () => check(false),
        show: () => setOpen(true),
      }}
    >
      {children}
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!installing) {
            setOpen(value);
            if (!value) dismissed.current = available?.version ?? null;
          }
        }}
      >
        <DialogContent showCloseButton={!installing}>
          <DialogTitle>
            {available
              ? `Tidy ${available.version} is available`
              : "Tidy updates"}
          </DialogTitle>
          <DialogDescription>
            {installing
              ? "Please keep Tidy open until the update finishes."
              : available
                ? "Install the update now, or come back to it in Settings. Tidy will save your changes and restart."
                : "You can close this window."}
          </DialogDescription>
          {!isTauri() && (
            <p className="text-xs text-text-muted">
              Browser preview. No software will be installed.
            </p>
          )}
          {!installing && available?.notes && (
            <p className="max-h-48 overflow-y-auto whitespace-pre-wrap text-sm text-text-muted">
              {available.notes}
            </p>
          )}
          {installing && (
            <p role="status" className="flex items-center gap-2">
              <Loader2 className="size-4 animate-spin" />
              {label}
            </p>
          )}
          {blocked && (
            <p className="text-sm text-text-muted">
              Finish recording or transcribing before installing.
            </p>
          )}
          {message && (phase === "error" || phase === "current") && (
            <p
              role={phase === "error" ? "alert" : "status"}
              className="text-sm"
            >
              {message}
            </p>
          )}
          {!installing && (
            <div className="flex justify-end gap-2">
              <Button
                variant="secondary"
                onClick={() => {
                  dismissed.current = available?.version ?? null;
                  setOpen(false);
                }}
              >
                {available ? "Later" : "Done"}
              </Button>
              {available && (
                <Button
                  disabled={!!blocked}
                  onClick={() => {
                    void install();
                  }}
                >
                  Install and restart
                </Button>
              )}
            </div>
          )}
        </DialogContent>
      </Dialog>
    </Context.Provider>
  );
}
export function UpdateSettings() {
  const updates = useUpdates();
  const busy = [
    "checking",
    "preparing",
    "downloading",
    "installing",
    "restarting",
  ].includes(updates.phase);
  return (
    <div className="space-y-3">
      <p className="text-sm text-text-muted">
        Version {version}. Tidy checks for updates when it opens and every six
        hours. Downloads only start when you choose to install.
      </p>
      <div className="flex gap-2">
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => {
            void updates.check();
          }}
        >
          {busy && <Loader2 className="size-4 animate-spin" />}
          {updates.phase === "checking" ? "Checking…" : "Check for updates"}
        </Button>
        {updates.available && (
          <Button onClick={updates.show}>View update</Button>
        )}
      </div>
      {updates.message && (
        <p
          role={updates.phase === "error" ? "alert" : "status"}
          className="text-sm text-text-muted"
        >
          {updates.message}
        </p>
      )}
    </div>
  );
}
