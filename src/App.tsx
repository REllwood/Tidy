import { UpdateProvider } from "@/features/updates/UpdateProvider";
import { AppShell } from "@/components/layout/AppShell";
import { CloseGuard } from "@/features/lifecycle/CloseGuard";

import { MeetingFlowProvider } from "@/features/meeting/MeetingFlowProvider";

function App() {
  return (
    <MeetingFlowProvider>
      <UpdateProvider>
        <AppShell />
        <CloseGuard />
      </UpdateProvider>
    </MeetingFlowProvider>
  );
}

export default App;
