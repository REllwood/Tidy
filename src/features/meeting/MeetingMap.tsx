import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Pause, Play } from "lucide-react";
import { isTauri } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { revealLine, usePlayback } from "./playback";
import { clock, type Turn } from "./meetingTimeline";

/** Something worth finding on the map: where a topic starts, a decision, a marked moment. */
export interface MapMark {
  at: number;
  kind: "topic" | "decision" | "moment";
  label: string;
}

function stampOf(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

/**
 * The meeting at a glance: who spoke when, coloured by speaker, with where
 * each topic starts, the decisions and marked moments above. It is also the
 * meeting's player: click anywhere to hear that moment and see its line in
 * the transcript.
 */
export function MeetingMap({
  pageId,
  turns,
  duration,
  colours,
  marks,
  audio,
}: {
  pageId: string;
  turns: Turn[];
  /** Seconds. */
  duration: number;
  colours: Map<string, string>;
  marks: MapMark[];
  /** Whether the meeting's audio was kept, so it can be played. */
  audio: boolean;
}) {
  const player = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [at, setAt] = useState(0);
  const request = usePlayback((s) => s.request);
  const playable = audio && isTauri();
  const length = Math.max(duration, 1);

  useEffect(() => {
    const el = player.current;
    if (!el || !request || request.pageId !== pageId) return;
    el.currentTime = request.seconds;
    setAt(request.seconds);
    void el.play().catch(() => undefined);
  }, [request, pageId]);

  const lineAt = (seconds: number) =>
    [...turns].reverse().find((t) => t.start <= seconds)?.start ?? turns[0]?.start ?? 0;

  const go = (seconds: number) => {
    const target = Math.min(Math.max(seconds, 0), length);
    revealLine(stampOf(lineAt(target)));
    setAt(target);
    const el = player.current;
    if (el) {
      el.currentTime = target;
      void el.play().catch(() => undefined);
    }
  };

  const onBar = (e: MouseEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    go(((e.clientX - box.left) / box.width) * length);
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 30 : 5;
    if (e.key === "ArrowRight") go(at + step);
    else if (e.key === "ArrowLeft") go(at - step);
    else if (e.key === "Home") go(0);
    else return;
    e.preventDefault();
  };
  const toggle = () => {
    const el = player.current;
    if (!el) return;
    if (el.paused) void el.play().catch(() => undefined);
    else el.pause();
  };
  const left = (seconds: number) => `${(Math.min(seconds, length) / length) * 100}%`;

  return (
    <div className="flex items-center gap-3">
      {playable && (
        <>
          <audio
            ref={player}
            preload="metadata"
            src={convertFileSrc(pageId, "tidy-audio")}
            onPlay={() => setPlaying(true)}
            onPause={() => setPlaying(false)}
            onTimeUpdate={(e) => setAt(e.currentTarget.currentTime)}
            className="hidden"
          />
          <button
            onClick={toggle}
            aria-label={playing ? "Pause the meeting" : "Play the meeting"}
            className="flex size-9 shrink-0 items-center justify-center rounded-full bg-brand text-brand-fg shadow-sm transition-transform hover:scale-105"
          >
            {playing ? (
              <Pause className="size-4 fill-current" />
            ) : (
              <Play className="ml-0.5 size-4 fill-current" />
            )}
          </button>
        </>
      )}
      <div className="min-w-0 flex-1">
        <div className="relative h-3">
          {marks.map((mark, i) => (
            <button
              key={i}
              onClick={() => go(mark.at)}
              title={`${mark.label} · ${clock(mark.at)}`}
              aria-label={`${mark.kind === "decision" ? "Decision" : mark.kind === "moment" ? "Marked moment" : "Topic"}: ${mark.label}, at ${clock(mark.at)}`}
              className="absolute top-0.5 -translate-x-1/2 p-0.5"
              style={{ left: left(mark.at) }}
            >
              <span
                className={cn(
                  "block size-2",
                  mark.kind === "decision" && "rotate-45 rounded-[1px] bg-brand",
                  mark.kind === "topic" && "rounded-full border-[1.5px] border-text-muted bg-surface",
                  mark.kind === "moment" && "rounded-full bg-rec",
                )}
              />
            </button>
          ))}
        </div>
        <div
          role="slider"
          tabIndex={0}
          aria-label="Meeting timeline"
          aria-valuemin={0}
          aria-valuemax={Math.round(length)}
          aria-valuenow={Math.round(at)}
          aria-valuetext={clock(at)}
          onClick={onBar}
          onKeyDown={onKey}
          className="relative mt-1 h-5 cursor-pointer rounded-md bg-bg-subtle outline-none focus-visible:ring-2 focus-visible:ring-brand"
        >
          {turns.map((turn, i) => (
            <span
              key={i}
              title={`${turn.speaker ?? "Speech"} · ${clock(turn.start)}`}
              className="absolute inset-y-0.5 rounded-[3px]"
              style={{
                left: left(turn.start),
                width: `max(2px, calc(${left(turn.end - turn.start)} - 1px))`,
                background: turn.speaker ? colours.get(turn.speaker) : "var(--text-faint)",
                opacity: turn.speaker ? 0.85 : 0.45,
              }}
            />
          ))}
          {(playing || at > 0) && (
            <span
              className="pointer-events-none absolute -inset-y-1 w-0.5 rounded-full bg-text"
              style={{ left: left(at) }}
              aria-hidden
            />
          )}
        </div>
        <div className="mt-1 flex justify-between font-mono text-2xs tabular-nums text-text-faint">
          <span>{clock(at)}</span>
          <span>{clock(length)}</span>
        </div>
      </div>
    </div>
  );
}
