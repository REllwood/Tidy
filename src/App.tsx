import { UpdateProvider } from "@/features/updates/UpdateProvider";
import { AppShell } from "@/components/layout/AppShell";

import { MeetingFlowProvider } from "@/features/meeting/MeetingFlowProvider";

function App() {
  return <MeetingFlowProvider><UpdateProvider><AppShell /></UpdateProvider></MeetingFlowProvider>;
}

export default App;
