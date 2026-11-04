import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarDays, Loader2 } from "lucide-react";
import { calendarApi } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";

export const calendarStatusKey = ["calendar-status"] as const;

/** Reading the Mac's calendar, which is off until the person turns it on. */
export function CalendarSettings() {
  const qc = useQueryClient();
  const status = useQuery({
    queryKey: calendarStatusKey,
    queryFn: calendarApi.status,
  });
  const connect = useMutation({
    mutationFn: calendarApi.connect,
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: calendarStatusKey });
      void qc.invalidateQueries({ queryKey: ["calendar-events"] });
    },
  });
  const disconnect = useMutation({
    mutationFn: calendarApi.disconnect,
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: calendarStatusKey });
      void qc.invalidateQueries({ queryKey: ["calendar-events"] });
    },
  });
  const on = status.data?.enabled && status.data.access === "granted";
  const denied = status.data?.access === "denied";
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-4">
        <span className="flex items-center gap-2 text-sm">
          <CalendarDays className="size-4 text-text-muted" />
          {on ? "Using your calendar" : "Calendar is off"}
        </span>
        {on ? (
          <Button
            size="sm"
            variant="outline"
            disabled={disconnect.isPending}
            onClick={() => disconnect.mutate()}
          >
            Turn off
          </Button>
        ) : (
          <Button
            size="sm"
            disabled={connect.isPending || status.isPending || denied}
            onClick={() => connect.mutate()}
          >
            {connect.isPending && <Loader2 className="size-4 animate-spin" />}
            Use my calendar
          </Button>
        )}
      </div>
      <p className="text-xs text-text-muted">
        Recordings are named after the meeting on your calendar, invitees are
        offered as speaker names, and a recording can stop 5 minutes after
        its event ends. Before a meeting, the recorder shows what earlier
        meetings in the same series decided and left open. Your calendar is
        read on this Mac only.
      </p>
      {denied && (
        <p role="status" className="text-xs text-warning">
          Calendar access is turned off for Tidy. Allow it under System
          Settings, Privacy and Security, Calendars, then come back here.
        </p>
      )}
      {(status.error || connect.error || disconnect.error) && (
        <p role="alert" className="text-sm text-danger-c">
          {errorMessage(status.error || connect.error || disconnect.error)}
        </p>
      )}
    </div>
  );
}
