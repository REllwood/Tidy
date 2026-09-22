import { useMemo } from "react";
import {
  Calendar,
  dateFnsLocalizer,
  type Event as RbcEvent,
} from "react-big-calendar";
import { format, parse, startOfWeek, getDay } from "date-fns";
import { enUS } from "date-fns/locale";
import { CalendarDays } from "lucide-react";
import "react-big-calendar/lib/css/react-big-calendar.css";
import "./calendar.css";
import type { DatabaseBundle, DbView } from "@/lib/api";
import { parseLocalDate } from "@/lib/dates";
import { isComputed, computeCell } from "@/lib/computed";
import { DbEmptyState, MissingFieldNotice } from "./EmptyState";
import { calendarDateField, titleField } from "./viewFields";

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
  targetBundles = {},
}: {
  bundle: DatabaseBundle;
  view: DbView;
  targetBundles?: Record<string, DatabaseBundle>;
}) {
  const { fields, rows } = bundle;
  const date = calendarDateField(fields, view.config);
  const dateField = date.field;
  const nameField = titleField(fields);

  const events = useMemo<RbcEvent[]>(() => {
    if (!dateField) return [];
    const out: RbcEvent[] = [];
    for (const r of rows) {
      // Date cells are "YYYY-MM-DD" with no zone: parse them as a LOCAL day, or
      // every event lands a day early west of UTC.
      const d = parseLocalDate(r.cells[dateField.id]);
      if (!d) continue;
      const raw = nameField
        ? isComputed(nameField.type)
          ? computeCell(nameField, r, fields, targetBundles)
          : r.cells[nameField.id]
        : null;
      out.push({
        title: raw == null || raw === "" ? "Untitled" : String(raw),
        start: d,
        end: d,
        allDay: true,
      });
    }
    return out;
  }, [rows, fields, dateField, nameField, targetBundles]);

  if (!dateField) {
    return (
      <div className="p-6">
        {date.missing && <MissingFieldNotice className="mb-4" />}
        <DbEmptyState
          icon={<CalendarDays className="size-5" />}
          title="No date field yet"
          hint="A calendar maps rows onto a Date field. Add one, then choose it in View settings."
        />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col gap-3 p-4">
      {date.missing && <MissingFieldNotice />}
      {events.length === 0 && (
        <DbEmptyState
          icon={<CalendarDays className="size-5" />}
          title="Nothing scheduled"
          hint={`No rows have a date in "${dateField.name}" yet.`}
        />
      )}
      <div className="tidy-calendar min-h-0 flex-1">
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
    </div>
  );
}
