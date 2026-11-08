import { create } from "zustand";
import { dateRange, type MeetingAnswer, type MeetingJob } from "@/lib/localAi";

// Keep the question when opening a source, without writing transcripts to browser storage.
export const useMeetingQuestion = create<{
  question: string;
  from: string;
  through: string;
  clientId: string;
  result: { question: string; scope: string; answer: MeetingAnswer } | null;
  update: (
    patch: Partial<{
      question: string;
      from: string;
      through: string;
      clientId: string;
      result: { question: string; scope: string; answer: MeetingAnswer } | null;
    }>,
  ) => void;
}>((set) => ({
  question: "",
  from: "",
  through: "",
  clientId: "",
  result: null,
  update: (patch) => set(patch),
}));

export function clientOptions(jobs: MeetingJob[]) {
  const clients = new Map<string, string>();
  for (const job of jobs)
    if (job.client_id && job.client_name)
      clients.set(job.client_id, job.client_name);
  return [...clients].sort((a, b) => a[1].localeCompare(b[1], "en-AU"));
}

export function scopedMeetings(
  jobs: MeetingJob[],
  clientId: string,
  from: string,
  through: string,
) {
  const range = dateRange(from, through);
  return jobs.filter(
    (job) =>
      (!clientId || job.client_id === clientId) &&
      (range.from === null || job.started_at >= range.from) &&
      (range.until === null || job.started_at < range.until),
  );
}

const COMMON = new Set(
  "the and for with what when who whom did does was were are our about from that this have has how why which last week weeks meeting meetings still there their they them into any all not yet can will would should could tell give me you your we us its it's".split(
    " ",
  ),
);

/** The words in a question worth looking for in meeting notes. */
export function searchWords(question: string): string[] {
  const words = question
    .toLowerCase()
    .split(/[^\p{L}\p{N}$]+/u)
    .filter((w) => w.length >= 3 && !COMMON.has(w));
  return [...new Set(words)];
}

/** A meeting whose notes or title mention the words, with the points that do. */
export interface MeetingMatch {
  job: MeetingJob;
  points: string[];
}

function mentions(text: string, word: string) {
  return new RegExp(`(^|[^\\p{L}\\p{N}])${word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`, "iu").test(text);
}

/**
 * Meetings that mention any of `words`, most matching points first within
 * each meeting, newest meeting first. With no words, every meeting, each
 * with the start of its overview.
 */
export function meetingMatches(jobs: MeetingJob[], words: string[]): MeetingMatch[] {
  const newest = [...jobs].sort((a, b) => b.started_at - a.started_at);
  if (!words.length)
    return newest.map((job) => ({
      job,
      points: job.summary?.summary ? [job.summary.summary] : [],
    }));
  return newest.flatMap((job) => {
    const s = job.summary;
    const points = s
      ? [
          s.summary,
          ...s.decisions,
          ...s.action_items,
          ...(s.open_questions ?? []),
          ...(s.sections ?? []).flatMap((x) => x.points),
        ].filter(Boolean)
      : [];
    const scored = points
      .map((point) => ({ point, hits: words.filter((w) => mentions(point, w)).length }))
      .filter((p) => p.hits > 0)
      .sort((a, b) => b.hits - a.hits)
      .slice(0, 3)
      .map((p) => p.point);
    const titled = words.some((w) => mentions(job.title, w));
    return scored.length || titled ? [{ job, points: scored }] : [];
  });
}
