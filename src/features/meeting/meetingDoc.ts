import type {
  TranscriptSegment,
  MeetingSummary,
  SpeakerSegment,
} from "@/lib/api";

export interface LabeledSegment extends TranscriptSegment {
  speaker?: number;
}

function mmss(ms: number): string {
  const total = Math.floor(ms / 1000);
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** Assign each transcript segment the speaker whose diarized span overlaps it most. */
export function assignSpeakers(
  transcript: TranscriptSegment[],
  speakers: SpeakerSegment[],
): LabeledSegment[] {
  return transcript.map((seg) => {
    let best = -1;
    let bestOverlap = 0;
    for (const sp of speakers) {
      const overlap =
        Math.min(seg.end_ms, sp.end_ms) - Math.max(seg.start_ms, sp.start_ms);
      if (overlap > bestOverlap) {
        bestOverlap = overlap;
        best = sp.speaker;
      }
    }
    return best >= 0 ? { ...seg, speaker: best } : { ...seg };
  });
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Block = Record<string, any>;

/** Compose a meeting note as BlockNote block JSON (the source of truth, not markdown). */
export function buildMeetingBlocks(
  segments: LabeledSegment[],
  summary: MeetingSummary | null,
  ollamaUsed: boolean,
): Block[] {
  const blocks: Block[] = [];

  if (summary && ollamaUsed) {
    blocks.push({ type: "heading", props: { level: 2 }, content: "Summary" });
    blocks.push({ type: "paragraph", content: summary.summary || "–" });

    if (summary.action_items.length) {
      blocks.push({ type: "heading", props: { level: 2 }, content: "Action items" });
      for (const a of summary.action_items)
        blocks.push({ type: "checkListItem", props: { checked: false }, content: a });
    }
    if (summary.decisions.length) {
      blocks.push({ type: "heading", props: { level: 2 }, content: "Decisions" });
      for (const d of summary.decisions)
        blocks.push({ type: "bulletListItem", content: d });
    }
  } else {
    blocks.push({
      type: "paragraph",
      content:
        "Summary skipped because Ollama wasn't running. Start Ollama to get AI summaries on your next recording.",
    });
  }

  blocks.push({ type: "heading", props: { level: 2 }, content: "Transcript" });
  if (segments.length === 0) {
    blocks.push({ type: "paragraph", content: "(No speech detected.)" });
  } else {
    const hasSpeakers = segments.some((s) => s.speaker != null);
    let lastSpeaker: number | undefined;
    for (const seg of segments) {
      if (hasSpeakers && seg.speaker != null && seg.speaker !== lastSpeaker) {
        lastSpeaker = seg.speaker;
        blocks.push({
          type: "paragraph",
          content: `Speaker ${seg.speaker + 1}`,
        });
      }
      blocks.push({
        type: "paragraph",
        content: `[${mmss(seg.start_ms)}] ${seg.text}`,
      });
    }
  }
  return blocks;
}
