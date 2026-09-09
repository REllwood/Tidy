import { AppShell } from "@/components/layout/AppShell";

import { MeetingFlowProvider } from "@/features/meeting/MeetingFlowProvider";

function App() {
  return <MeetingFlowProvider><AppShell /></MeetingFlowProvider>;
}

export default App;
