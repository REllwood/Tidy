import { createContext, useContext, useEffect, useRef, type ReactNode } from "react";
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
  return <Context.Provider value={flow}>{children}</Context.Provider>;
}
export function useSharedMeetingFlow() {
  const flow = useContext(Context);
  if (!flow)
    throw new Error("Meeting controls must be inside MeetingFlowProvider");
  return flow;
}
