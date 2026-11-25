import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { liveApi, liveFeedApi, type AgentNote, type LiveLine, type LiveNotes, type LiveNotesBy } from "@/lib/api";
import { errorMessage } from "@/lib/errors";
import { isTauri } from "@/lib/tauri";
import { useUi } from "@/store/ui";

/**
 * How often the notes update by themselves: every minute and a half on this
 * Mac, and every three minutes from Claude or Codex, which use the person's
 * plan or quota on each update. Either way only once this much more has
 * been said.
 */
export const UPDATE_EVERY_MS: Record<LiveNotesBy, number> = { local: 90_000, claude: 180_000, codex: 180_000 };
export const UPDATE_AFTER_CHARS = 300;
const CHECK_EVERY_MS = 15_000;

/** Whether the notes are due an update by themselves. */
export function updateDue(now: number, lastRun: number, heard: number, heardAtLast: number, every: number): boolean {
  return now - lastRun >= every && heard - heardAtLast >= UPDATE_AFTER_CHARS;
}

/** Errors an update made by itself keeps quiet about: it tries again later. */
function quiet(message: string): boolean {
  return /cancelled|already being updated|Another local AI operation|Nothing has been transcribed/i.test(message);
}

export interface LiveNotesState {
  notes: LiveNotes | null;
  error: string | null;
  busy: boolean;
  /** Who writes them now: the chosen writer, or this Mac when that is not available. */
  by: LiveNotesBy;
  /** Whether Claude or Codex can be chosen. */
  can: (by: LiveNotesBy) => boolean;
  sharing: boolean;
  agentNotes: AgentNote[];
  /** The clock the "updated … ago" text is read against. */
  now: number;
  update: (byHand: boolean) => Promise<void>;
}

/**
 * Live notes for the whole recording, whichever page or tab is open: they
 * update by themselves while the meeting is recorded and are kept until the
 * next recording starts.
 */
export function useLiveNotes(lines: LiveLine[], recording: boolean): LiveNotesState {
  const auto = useUi((s) => s.liveNotesAuto);
  const views = useUi((s) => s.liveNotesViews);
  const custom = useUi((s) => s.liveNotesCustom);
  const chosenBy = useUi((s) => s.liveNotesBy);

  const feed = useQuery({
    queryKey: ["live-feed-status"],
    queryFn: liveFeedApi.status,
    staleTime: 30_000,
    enabled: recording,
  });
  const sharing = !!feed.data?.enabled;
  const can = useCallback(
    (by: LiveNotesBy) => by === "local" || (!!feed.data?.enabled && !!feed.data?.[by]),
    [feed.data],
  );
  const by: LiveNotesBy = can(chosenBy) ? chosenBy : "local";

  const [notes, setNotes] = useState<LiveNotes | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [agentNotes, setAgentNotes] = useState<AgentNote[]>([]);
  const running = useRef(false);
  const lastRun = useRef(0);
  const heardAtLast = useRef(0);
  const lastAgentNote = useRef(0);
  const heard = useMemo(() => lines.reduce((n, l) => n + l.text.length, 0), [lines]);

  // A recording starts, or Tidy reconnects to one: start from what the
  // app already has, which is nothing for a new recording.
  useEffect(() => {
    if (!recording) return;
    setNotes(null);
    setError(null);
    setAgentNotes([]);
    lastRun.current = Date.now();
    heardAtLast.current = 0;
    lastAgentNote.current = 0;
    let gone = false;
    liveApi
      .latest()
      .then((n) => {
        if (gone) return;
        if (n) {
          setNotes(n);
          lastRun.current = n.updated_at;
        } else {
          // Nothing yet: the first update comes once enough has been said.
          lastRun.current = 0;
        }
      })
      .catch(() => {
        if (!gone) lastRun.current = 0;
      });
    return () => {
      gone = true;
    };
  }, [recording]);

  const asked = useMemo(() => views.filter((v) => v !== "custom" || custom.trim()), [views, custom]);
  const update = useCallback(
    async (byHand: boolean) => {
      if (running.current || asked.length === 0) return;
      running.current = true;
      setBusy(true);
      const heardNow = heard;
      try {
        const next = await liveApi.notes(asked, custom.trim() || null, by);
        setNotes(next);
        setError(null);
        heardAtLast.current = heardNow;
      } catch (e) {
        const message = errorMessage(e);
        if (byHand || !quiet(message)) setError(message);
      } finally {
        lastRun.current = Date.now();
        running.current = false;
        setBusy(false);
      }
    },
    [asked, custom, by, heard],
  );

  // Updates by themselves, once enough new speech has come in.
  const latest = useRef({ update, heard, by });
  latest.current = { update, heard, by };
  useEffect(() => {
    if (!recording) return;
    const tick = () => {
      setNow(Date.now());
      const { update, heard, by } = latest.current;
      if (auto && updateDue(Date.now(), lastRun.current, heard, heardAtLast.current, UPDATE_EVERY_MS[by])) {
        void update(false);
      }
    };
    const id = setInterval(tick, CHECK_EVERY_MS);
    return () => clearInterval(id);
  }, [auto, recording]);

  // Notes Claude or Codex post through Tidy's MCP server.
  useEffect(() => {
    if (!sharing || !recording) return;
    let gone = false;
    const fetchNew = () =>
      liveApi
        .agentNotes(lastAgentNote.current)
        .then((more) => {
          if (gone || more.length === 0) return;
          lastAgentNote.current = more[more.length - 1].id;
          setAgentNotes((all) => [...all, ...more]);
        })
        .catch(() => {});
    void fetchNew();
    let stop: (() => void) | undefined;
    if (isTauri()) {
      void listen("live-notes-posted", () => void fetchNew()).then((un) => {
        if (gone) un();
        else stop = un;
      });
    }
    return () => {
      gone = true;
      stop?.();
    };
  }, [sharing, recording]);

  return { notes, error, busy, by, can, sharing, agentNotes, now, update };
}

export const LiveNotesContext = createContext<LiveNotesState | null>(null);

export function useSharedLiveNotes(): LiveNotesState {
  const notes = useContext(LiveNotesContext);
  if (!notes) throw new Error("Live notes must be inside MeetingFlowProvider");
  return notes;
}
