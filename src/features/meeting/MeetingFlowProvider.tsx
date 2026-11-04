import { createContext, useContext, useEffect, useRef, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@/lib/tauri";
import { useMeetingFlow } from "./useMeetingFlow";
const Context = createContext<ReturnType<typeof useMeetingFlow> | null>(null);
export function MeetingFlowProvider({ children }: { children: ReactNode }) {
  const flow = useMeetingFlow();
  const { reconcile } = flow;
  // Ask the core once whether a recording is already running, so reloading the
  // webview mid-meeting reconnects the recorder instead of orphaning it.
  const checked = useRef(false);
  useEffect(() => {
    if (checked.current) return;
    checked.current = true;
    void reconcile();
  }, [reconcile]);
  // The floating meeting window asks to record a call it spotted, and the
  // call monitor stops a recording once the meeting has clearly finished.
  const latest = useRef(flow);
  latest.current = flow;
  useEffect(() => {
    if (!isTauri()) return;
    const handles = [
      listen<string>("hud-record", () => {
        const { state, start } = latest.current;
        if (["idle", "done", "error"].includes(state.phase)) void start();
      }),
      listen<string>("auto-stop", (e) => {
        const { state, stop } = latest.current;
        if (state.phase === "recording") void stop(e.payload);
      }),
    ];
    return () => {
      for (const handle of handles) void handle.then((unlisten) => unlisten());
    };
  }, []);
  return <Context.Provider value={flow}>{children}</Context.Provider>;
}
export function useSharedMeetingFlow() {
  const flow = useContext(Context);
  if (!flow)
    throw new Error("Meeting controls must be inside MeetingFlowProvider");
  return flow;
}
