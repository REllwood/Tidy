import { create } from "zustand";

/** "00:12:34" or "12:34" as seconds. */
export function stampSeconds(stamp: string): number | null {
  const parts = stamp.trim().split(":").map(Number);
  if (parts.length < 2 || parts.length > 3 || parts.some((n) => !Number.isFinite(n)))
    return null;
  return parts.reduce((total, n) => total * 60 + n, 0);
}

/** A stamp as it reads best: "12:34", or "1:02:03" past an hour. */
export function shortStamp(stamp: string): string {
  const seconds = stampSeconds(stamp);
  if (seconds === null) return stamp;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor(seconds / 60) % 60;
  const s = seconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** The "[hh:mm:ss]" a transcript line starts with, if it has one. */
export function lineStamp(text: string): string | null {
  return /^\s*\[(\d{1,2}(?::\d{2}){1,2})\]/.exec(text)?.[1] ?? null;
}

interface Playback {
  /** The latest request to play a meeting from a point. */
  request: { pageId: string; seconds: number; nonce: number } | null;
  play: (pageId: string, seconds: number) => void;
}

/** Lets notes and transcript lines ask the meeting's player to jump. */
export const usePlayback = create<Playback>((set) => ({
  request: null,
  play: (pageId, seconds) =>
    set({ request: { pageId, seconds, nonce: Date.now() + Math.random() } }),
}));

const FOUND = "transcript-line-found";
let foundTimer: ReturnType<typeof setTimeout> | undefined;

/** Scroll the page to the transcript line said at `stamp`, briefly marked. */
export function revealLine(stamp: string) {
  const seconds = stampSeconds(stamp);
  if (seconds === null) return;
  const lines = document.querySelectorAll<HTMLElement>(".bn-inline-content");
  for (const line of lines) {
    const found = lineStamp(line.textContent ?? "");
    if (found && stampSeconds(found) === seconds) {
      line.scrollIntoView({ block: "center", behavior: "smooth" });
      // Marked as a text highlight: the editor redraws a line whose markup
      // changes, which would drop a class at once.
      const highlights = (globalThis.CSS as { highlights?: Map<string, unknown> } | undefined)?.highlights;
      if (highlights && typeof Highlight !== "undefined") {
        const range = document.createRange();
        range.selectNodeContents(line);
        highlights.set(FOUND, new Highlight(range));
        clearTimeout(foundTimer);
        foundTimer = setTimeout(() => highlights.delete(FOUND), 1800);
      }
      return;
    }
  }
}

/**
 * The transcript stamp under a click, when the click lands on the stamp
 * itself rather than the words after it.
 */
export function stampUnder(x: number, y: number): string | null {
  const caret = document.caretRangeFromPoint?.(x, y);
  const line = caret?.startContainer.parentElement?.closest(".bn-inline-content");
  if (!caret || !line) return null;
  const stamp = lineStamp(line.textContent ?? "");
  if (!stamp) return null;
  const before = document.createRange();
  before.setStart(line, 0);
  before.setEnd(caret.startContainer, caret.startOffset);
  return before.toString().length <= stamp.length + 2 ? stamp : null;
}

/** Scroll the page to its "Transcript" heading. */
export function revealTranscript() {
  const headings = document.querySelectorAll<HTMLElement>(
    ".tidy-editor [data-content-type='heading'] .bn-inline-content",
  );
  for (const heading of headings) {
    if (heading.textContent?.trim().toLowerCase() === "transcript") {
      heading.scrollIntoView({ block: "start", behavior: "smooth" });
      return;
    }
  }
}
