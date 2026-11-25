import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Loader2, MessageCircleQuestion, NotebookPen, ScrollText } from "lucide-react";
import type { LiveLine } from "@/lib/api";
import { localAi, type MeetingAnswer } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { MeetingAnswerText } from "./MeetingAnswerText";
import { LiveNotesPanel } from "./LiveNotesPanel";

type Tab = "transcript" | "notes" | "ask";

/**
 * Who said a live line: "You" and the person the call window showed
 * speaking, or "Others", on a call. Nobody in a room.
 */
export function liveSpeaker(line: LiveLine, lines: LiveLine[]): string | null {
  if (line.source === "system") return line.speaker || "Others";
  return lines.some((l) => l.source === "system") ? "You" : null;
}

/**
 * Add a live line, or replace the one it names again: a call line is sent
 * again once the call window shows who said it.
 */
export function withLiveLine(lines: LiveLine[], line: LiveLine): LiveLine[] {
  const at = lines.findIndex((l) => l.source === line.source && l.start_ms === line.start_ms);
  if (at === -1) return [...lines, line];
  return lines.map((l, i) => (i === at ? line : l));
}

function clock(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * Help during the meeting: the live transcript, a recap of the meeting so
 * far, and answers from earlier meetings, all without leaving the recorder.
 */
export function LivePanel({ lines }: { lines: LiveLine[] }) {
  const [tab, setTab] = useState<Tab>("transcript");
  const sorted = [...lines].sort((a, b) => a.start_ms - b.start_ms);
  const end = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  // A line jumped to from the notes, by when it started.
  const [found, setFound] = useState<number | null>(null);
  useEffect(() => {
    if (tab === "transcript" && found == null) end.current?.scrollIntoView({ block: "nearest" });
  }, [lines.length, tab, found]);
  useEffect(() => {
    if (found == null || tab !== "transcript") return;
    list.current?.querySelector(`[data-start="${found}"]`)?.scrollIntoView({ block: "center" });
    const id = setTimeout(() => setFound(null), 4000);
    return () => clearTimeout(id);
  }, [found, tab]);
  const showLine = (atMs: number) => {
    // Notes know the second a line started; the first line that second.
    const line = sorted.find((l) => Math.floor(l.start_ms / 1000) * 1000 >= atMs) ?? sorted[sorted.length - 1];
    if (!line) return;
    setFound(line.start_ms);
    setTab("transcript");
  };

  const [question, setQuestion] = useState("");
  const ask = useMutation({
    mutationFn: (q: string) => localAi.ask(q, null, null, null),
  });

  const tabs: { id: Tab; label: string; icon: typeof ScrollText }[] = [
    { id: "transcript", label: "Transcript", icon: ScrollText },
    { id: "notes", label: "Live notes", icon: NotebookPen },
    { id: "ask", label: "Ask past meetings", icon: MessageCircleQuestion },
  ];

  return (
    <div className="mt-5 border-t border-border pt-4">
      <div role="tablist" className="mb-4 inline-flex rounded-full bg-bg-subtle p-0.5">
        {tabs.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={`flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium transition-colors ${tab === id ? "bg-surface text-text shadow-sm" : "text-text-muted hover:text-text"}`}
          >
            <Icon className="size-3.5" />
            {label}
          </button>
        ))}
      </div>

      {tab === "transcript" && (
        <div ref={list} className="scroll-fade max-h-72 space-y-1 overflow-y-auto">
          {sorted.length === 0 && (
            <p className="text-sm text-text-faint">
              Lines appear here a moment after someone finishes speaking.
            </p>
          )}
          {sorted.map((line, i) => {
            const who = liveSpeaker(line, sorted);
            const same = i > 0 && liveSpeaker(sorted[i - 1], sorted) === who;
            return (
              <div
                key={`${line.source}-${line.start_ms}-${i}`}
                data-start={line.start_ms}
                className={`grid grid-cols-[7rem_1fr] gap-x-3 rounded transition-colors ${same ? "" : "pt-1.5"} ${found === line.start_ms ? "bg-brand-soft" : ""}`}
                title={clock(line.start_ms)}
              >
                <span
                  className={`truncate pt-0.5 text-xs font-medium ${who === "You" ? "text-brand" : "text-text-faint"}`}
                  title={same || !who ? undefined : who}
                >
                  {same ? "" : (who ?? clock(line.start_ms))}
                </span>
                <p className="text-sm leading-6 text-text">{line.text}</p>
              </div>
            );
          })}
          <div ref={end} className="h-4" aria-hidden />
        </div>
      )}

      {tab === "notes" && <LiveNotesPanel lines={sorted} onShowLine={showLine} />}

      {tab === "ask" && (
        <form
          className="space-y-3 text-sm"
          onSubmit={(e) => {
            e.preventDefault();
            if (question.trim()) ask.mutate(question.trim());
          }}
        >
          <div className="flex gap-2">
            <input
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="What did we decide about the launch last time?"
              className="min-w-0 flex-1 rounded-lg border border-border bg-bg px-3 py-1.5 text-sm outline-none focus:border-brand"
              aria-label="Question about earlier meetings"
            />
            <Button size="sm" type="submit" disabled={ask.isPending || !question.trim()}>
              {ask.isPending && <Loader2 className="size-3 animate-spin" />}
              Ask
            </Button>
          </div>
          {ask.error && (
            <p role="alert" className="text-danger-c">
              {errorMessage(ask.error)}
            </p>
          )}
          {ask.data && <AnswerView answer={ask.data} />}
        </form>
      )}
    </div>
  );
}

function AnswerView({ answer }: { answer: MeetingAnswer }) {
  return (
    <div className="space-y-2">
      <MeetingAnswerText text={answer.answer} sourceCount={answer.sources.length} />
      {answer.sources.length > 0 && (
        <p className="text-xs text-text-faint">
          From {new Set(answer.sources.map((s) => s.page_id)).size} earlier{" "}
          {new Set(answer.sources.map((s) => s.page_id)).size === 1 ? "meeting" : "meetings"}.
          Open Ask meetings for the sources.
        </p>
      )}
    </div>
  );
}
