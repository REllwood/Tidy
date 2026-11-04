import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { queryClient } from "@/lib/queryClient";
import App from "./App";
import { MeetingHud } from "@/features/meeting/MeetingHud";
import "@fontsource-variable/geist";
import "./index.css";

// The floating meeting window loads the same bundle with ?view=hud.
const hud = new URLSearchParams(window.location.search).get("view") === "hud";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  hud ? (
    <React.StrictMode>
      <MeetingHud />
    </React.StrictMode>
  ) : (
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <App />
        <Toaster />
      </TooltipProvider>
    </QueryClientProvider>
  </React.StrictMode>
  ),
);
