import { createContext, useContext, type ReactNode } from "react";
import { useMeetingFlow } from "./useMeetingFlow";
const Context = createContext<ReturnType<typeof useMeetingFlow> | null>(null);
export function MeetingFlowProvider({ children }: { children: ReactNode }) {
  const flow = useMeetingFlow();
  return <Context.Provider value={flow}>{children}</Context.Provider>;
}
export function useSharedMeetingFlow() {
  const flow = useContext(Context);
  if (!flow)
    throw new Error("Meeting controls must be inside MeetingFlowProvider");
  return flow;
}
