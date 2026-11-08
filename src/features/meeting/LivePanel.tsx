import { useEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { Loader2, MessageCircleQuestion, ScrollText, Sparkles } from "lucide-react";
import { liveApi, type LiveLine, type LiveRecap } from "@/lib/api";
import { localAi, type MeetingAnswer } from "@/lib/localAi";
import { errorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import { MeetingAnswerText } from "./MeetingAnswerText";

type Tab = "transcript" | "recap" | "ask";

/** Who said a live line: "You" and "Others" on a call, nobody in a room. */
export function liveSpeaker(line: LiveLine, lines: LiveLine[]): string | null {
  if (line.source === "system") return "Others";
  return lines.some((l) => l.source === "system") ? "You" : null;
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
  useEffect(() => {
    if (tab === "transcript") end.current?.scrollIntoView({ block: "nearest" });
  }, [lines.length, tab]);

  const recap = useMutation({ mutationFn: liveApi.recap });
  const [question, setQuestion] = useState("");
  const ask = useMutation({
    mutationFn: (q: string) => localAi.ask(q, null, null, null),
  });

  const tabs: { id: Tab; label: string; icon: typeof ScrollText }[] = [
    { id: "transcript", label: "Transcript", icon: ScrollText },
    { id: "recap", label: "Recap so far", icon: Sparkles },
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
        <div className="max-h-72 space-y-1 overflow-y-auto">
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
                className={`grid grid-cols-[4rem_1fr] gap-x-3 ${same ? "" : "pt-1.5"}`}
                title={clock(line.start_ms)}
              >
                <span
                  className={`pt-0.5 text-xs font-medium ${who === "You" ? "text-brand" : "text-text-faint"}`}
                >
                  {same ? "" : (who ?? clock(line.start_ms))}
                </span>
                <p className="text-sm leading-6 text-text">{line.text}</p>
              </div>
            );
          })}
          <div ref={end} />
        </div>
      )}

      {tab === "recap" && (
        <div className="space-y-3 text-sm">
          <Button
            size="sm"
            variant="secondary"
            disabled={recap.isPending || lines.length === 0}
            onClick={() => recap.mutate()}
          >
            {recap.isPending && <Loader2 className="size-3 animate-spin" />}
            {recap.data ? "Update recap" : "Recap the meeting so far"}
          </Button>
          {recap.error && (
            <p role="alert" className="text-danger-c">
              {errorMessage(recap.error)}
            </p>
          )}
          {recap.data && <RecapView recap={recap.data} />}
          <p className="text-xs text-text-faint">
            Written on this Mac from the live transcript. Each point is checked
            against what was said.
          </p>
        </div>
      )}

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

function RecapView({ recap }: { recap: LiveRecap }) {
  const groups = [
    { title: "So far", points: recap.points },
    { title: "Decisions", points: recap.decisions },
    { title: "Next steps", points: recap.actions },
    { title: "Still open", points: recap.open_questions },
  ].filter((g) => g.points.length);
  if (!groups.length)
    return (
      <p className="text-text-muted">
        Nothing clear enough to recap yet. Try again in a few minutes.
      </p>
    );
  return (
    <div className="space-y-3">
      {groups.map((g) => (
        <section key={g.title}>
          <h4 className="text-xs font-semibold text-text">{g.title}</h4>
          <ul className="mt-1 list-disc space-y-1 pl-5 text-text-muted marker:text-brand">
            {g.points.map((p, i) => (
              <li key={i}>{p}</li>
            ))}
          </ul>
        </section>
      ))}
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
