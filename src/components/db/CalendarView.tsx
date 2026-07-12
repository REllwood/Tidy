import { useMemo } from "react";
import {
  Calendar,
  dateFnsLocalizer,
  type Event as RbcEvent,
} from "react-big-calendar";
import { format, parse, startOfWeek, getDay } from "date-fns";
import { enUS } from "date-fns/locale";
import "react-big-calendar/lib/css/react-big-calendar.css";
import "./calendar.css";
import type { DatabaseBundle, DbView } from "@/lib/api";

const localizer = dateFnsLocalizer({
  format,
  parse,
  startOfWeek: () => startOfWeek(new Date(), { weekStartsOn: 0 }),
  getDay,
  locales: { "en-US": enUS },
});

export function CalendarView({
  bundle,
  view,
}: {
  bundle: DatabaseBundle;
  view: DbView;
}) {
  const { fields, rows } = bundle;
  const dateField = fields.find((f) => f.id === view.config?.dateFieldId);
  const nameField = fields.find((f) => f.type === "text") ?? fields[0];

  const events = useMemo<RbcEvent[]>(() => {
    if (!dateField) return [];
    return rows
      .filter((r) => r.cells[dateField.id])
      .map((r) => {
        // Parse "YYYY-MM-DD" in LOCAL time so the event lands on the right day
        // (new Date("YYYY-MM-DD") parses as UTC midnight → off-by-one west of UTC).
        const raw = String(r.cells[dateField.id]);
        const [y, m, dd] = raw.split("-").map(Number);
        const d =
          y && m && dd ? new Date(y, m - 1, dd) : new Date(raw);
        return {
          title: r.cells[nameField.id] || "Untitled",
          start: d,
          end: d,
          allDay: true,
        };
      });
  }, [rows, dateField, nameField]);

  if (!dateField) {
    return (
      <div className="p-8 text-sm text-text-faint">
        Pick a <b>Date</b> field to map rows onto the calendar.
      </div>
    );
  }

  return (
    <div className="appflower-calendar h-full p-4">
      <Calendar
        localizer={localizer}
        events={events}
        defaultView="month"
        views={["month"]}
        startAccessor="start"
        endAccessor="end"
        style={{ height: "100%" }}
      />
    </div>
  );
}
