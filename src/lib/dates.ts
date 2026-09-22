/**
 * Local-calendar date helpers shared by the database views.
 *
 * Every date cell in Tidy is a plain "YYYY-MM-DD" string with no timezone, so
 * it has to be read as a LOCAL calendar day. `new Date("2026-06-20")` parses as
 * UTC midnight, which renders as the 19th anywhere west of UTC, so nothing here
 * goes through that path.
 */

/** Parse "YYYY-MM-DD" (or any date string) in LOCAL time. */
export function parseLocalDate(v: unknown): Date | null {
  if (v == null || v === "") return null;
  if (v instanceof Date) return isNaN(v.getTime()) ? null : v;
  const s = String(v);
  const m = s.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (m) return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  const d = new Date(s);
  return isNaN(d.getTime()) ? null : d;
}

/**
 * Index of the LOCAL calendar day a date falls on. Bucketing on the raw epoch
 * would shift the boundary by the zone offset, so "today" lands on the wrong
 * column east or west of UTC; subtracting the offset moves local midnight onto
 * a UTC day boundary first.
 */
export function dayNum(d: Date): number {
  return Math.floor(
    (d.getTime() - d.getTimezoneOffset() * 60_000) / 86_400_000,
  );
}

/** A date `n` local days after `d` (negative `n` goes back). */
export function addDays(d: Date, n: number): Date {
  const x = new Date(d);
  x.setDate(x.getDate() + n);
  return x;
}

/** Day index of a raw cell value, or null when it isn't a date. */
export function dayKey(v: unknown): number | null {
  const d = parseLocalDate(v);
  return d ? dayNum(d) : null;
}

/** "YYYY-MM-DD" for a local date (the shape date cells are stored in). */
export function toIsoDay(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
