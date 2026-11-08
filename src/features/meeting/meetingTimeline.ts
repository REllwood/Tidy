import { lineStamp, stampSeconds } from "./playback";

/** A stretch of the meeting one person spoke, in seconds from the start. */
export interface Turn {
  /** Their name or label, or null when the transcript does not say. */
  speaker: string | null;
  start: number;
  end: number;
}

/** How much of the meeting someone spoke. */
export interface Talker {
  speaker: string;
  seconds: number;
  /** Whole percent of all the speech. */
  share: number;
}

/** A line runs until the next one, but a pause this long is not speech. */
const LONGEST_LINE = 30;
/** Lines from one person closer than this are one turn on the map. */
const SAME_TURN_GAP = 5;

function text(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(text).join("");
  if (value && typeof value === "object" && "text" in value)
    return text((value as { text: unknown }).text);
  return "";
}

/**
 * Who spoke when, from the page's transcript: a speaker's name sits on its
 * own line before their "[hh:mm:ss] …" lines. `duration` (seconds) ends the
 * last line.
 */
export function transcriptTurns(body: string, duration?: number): Turn[] {
  let blocks: unknown;
  try {
    blocks = JSON.parse(body);
  } catch {
    return [];
  }
  if (!Array.isArray(blocks)) return [];
  const lines = blocks.map((b) =>
    b && typeof b === "object" ? text((b as { content?: unknown }).content).trim() : "",
  );
  const heading = blocks.findIndex(
    (b, i) =>
      b && typeof b === "object" &&
      (b as { type?: string }).type === "heading" &&
      lines[i].toLowerCase() === "transcript",
  );
  const said: { speaker: string | null; start: number }[] = [];
  let speaker: string | null = null;
  for (let i = heading + 1; i < lines.length; i++) {
    const line = lines[i];
    const stamp = lineStamp(line);
    const seconds = stamp ? stampSeconds(stamp) : null;
    if (seconds !== null) {
      said.push({ speaker, start: seconds });
    } else if (line && line.length <= 40 && lineStamp(lines[i + 1] ?? "")) {
      speaker = line;
    }
  }
  const lineTurns = said.map((line, i) => {
    const next = said[i + 1]?.start ?? duration ?? line.start + 10;
    const end = Math.min(Math.max(next, line.start + 1), line.start + LONGEST_LINE);
    return { ...line, end };
  });
  const turns: Turn[] = [];
  for (const line of lineTurns) {
    const last = turns[turns.length - 1];
    if (last && last.speaker === line.speaker && line.start - last.end <= SAME_TURN_GAP)
      last.end = Math.max(last.end, line.end);
    else turns.push({ ...line });
  }
  return turns;
}

/** Each named speaker's share of the talking, most first. */
export function talkShare(turns: Turn[]): Talker[] {
  const seconds = new Map<string, number>();
  for (const turn of turns) {
    if (!turn.speaker) continue;
    seconds.set(turn.speaker, (seconds.get(turn.speaker) ?? 0) + turn.end - turn.start);
  }
  const total = [...seconds.values()].reduce((a, b) => a + b, 0);
  if (!total) return [];
  return [...seconds.entries()]
    .map(([speaker, s]) => ({ speaker, seconds: s, share: Math.round((s / total) * 100) }))
    .sort((a, b) => b.seconds - a.seconds);
}

/**
 * A colour for each speaker that stays the same across the map and the
 * people list. The person recording always has the brand colour.
 */
export function speakerColours(speakers: string[]): Map<string, string> {
  const colours = new Map<string, string>();
  let next = 1;
  for (const speaker of speakers) {
    if (colours.has(speaker)) continue;
    if (speaker === "You") colours.set(speaker, "var(--brand)");
    else colours.set(speaker, `var(--speaker-${((next++ - 1) % 6) + 1})`);
  }
  return colours;
}

/** A next step taken apart: who, what and by when, when the text says. */
export interface NextStep {
  owner: string | null;
  task: string;
  due: string | null;
}

/** "Priya will fix the form by Thursday." → Priya, "Fix the form", "by Thursday". */
export function nextStepParts(text: string): NextStep {
  const whole = text.trim();
  const owned = /^([^,.;]{1,40}?) will (.+)$/i.exec(whole);
  const owner = owned && owned[1].split(/\s+/).length <= 4 ? owned[1] : null;
  let task = owner && owned ? owned[2] : whole;
  let due: string | null = null;
  const dated = /\s((?:by|before)\s+[^,;]+?)\.?$/i.exec(task);
  if (dated && dated.index > 0) {
    due = dated[1];
    task = task.slice(0, dated.index);
  }
  task = task.replace(/[.\s]+$/, "");
  if (owner) task = task.charAt(0).toUpperCase() + task.slice(1);
  return { owner, task, due };
}

/** "0:26" or "1:02:03": short times for the notes, from "00:00:26". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor(s / 60) % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

/** The earliest time any of `points` was said. */
export function firstTime(points: string[], times: Record<string, string> | undefined): number | null {
  const found = points
    .map((p) => (times?.[p] ? stampSeconds(times[p]) : null))
    .filter((s): s is number => s !== null);
  return found.length ? Math.min(...found) : null;
}
