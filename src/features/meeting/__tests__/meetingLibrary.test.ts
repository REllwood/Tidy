import { describe, expect, it } from "vitest";
import {
  clientOptions,
  scopedMeetings,
  useMeetingQuestion,
} from "../meetingLibrary";
import type { MeetingJob } from "@/lib/localAi";
const job = (id: string, client: string, date: string): MeetingJob => ({
  page_id: id,
  title: id,
  client_id: client,
  client_name: client,
  started_at: new Date(`${date}T12:00:00`).getTime(),
  indexed: true,
  passage_count: 2,
  state: "ready",
  error: null,
  summary: null,
});
const jobs = [
  job("first", "Acme", "2026-09-01"),
  job("second", "Acme", "2026-09-09"),
  job("private", "Other", "2026-09-09"),
];
describe("client meeting library", () => {
  it("combines the same client's meetings and keeps other clients outside the scope", () => {
    expect(scopedMeetings(jobs, "Acme", "", "").map((j) => j.page_id)).toEqual([
      "first",
      "second",
    ]);
    expect(
      scopedMeetings(jobs, "Acme", "2026-09-09", "2026-09-09").map(
        (j) => j.page_id,
      ),
    ).toEqual(["second"]);
    expect(scopedMeetings(jobs, "missing", "", "")).toEqual([]);
    expect(scopedMeetings(jobs, "", "", "")).toHaveLength(3);
    expect(clientOptions(jobs)).toEqual([
      ["Acme", "Acme"],
      ["Other", "Other"],
    ]);
  });
  it("retains the answer and its original scope while opening source notes", () => {
    const result = {
      question: "What changed?",
      scope: "Acme",
      answer: { answer: "A revision", sources: [] },
    };
    useMeetingQuestion
      .getState()
      .update({ question: result.question, clientId: "Acme", result });
    useMeetingQuestion.getState().update({ clientId: "Other" });
    expect(useMeetingQuestion.getState().result).toEqual(result);
  });
});
