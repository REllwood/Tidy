import type { QueryClient } from "@tanstack/react-query";
import { recordingHistoryKey } from "@/lib/api";
import { aiStatusKey, meetingJobsKey } from "@/lib/localAi";

/**
 * Everything a finished (or failed) recording can change. Listed explicitly so
 * the recorder never resets the whole query cache, which would refetch every
 * open board, calendar and document at once.
 */
export function meetingInvalidationKeys(
  pageId: string | null,
): readonly (readonly unknown[])[] {
  return [
    ["pages"],
    recordingHistoryKey,
    meetingJobsKey,
    aiStatusKey,
    ["database"],
    ["database-by-id"],
    ...(pageId
      ? [
          ["document", pageId],
          ["transcript-versions", pageId],
        ]
      : []),
  ];
}

export function invalidateMeetingQueries(qc: QueryClient, pageId: string | null) {
  for (const queryKey of meetingInvalidationKeys(pageId))
    void qc.invalidateQueries({ queryKey: [...queryKey] });
}
