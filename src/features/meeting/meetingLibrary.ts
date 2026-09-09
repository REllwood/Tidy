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
