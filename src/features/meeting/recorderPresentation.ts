import type { MeetingPhase, MeetingState } from "./useMeetingFlow";

/**
 * Which retention message the recorder shows. Once a recording exists, the
 * value the core returned for *this* meeting wins over the saved preference,
 * because changing the setting mid-meeting does not change the meeting. Before
 * the core has answered ("idle" and "starting") the preference is all we have.
 */
export function keepAudioNotice(
  phase: MeetingPhase,
  keepAudio: boolean,
  preference: boolean | undefined,
): boolean | undefined {
  return phase === "idle" || phase === "starting" ? preference : keepAudio;
}

export type RecorderControl = "start" | "cancel" | "stop" | "reset";

/** Exactly one primary control is live in each phase. */
export function controlEnabled(
  control: RecorderControl,
  phase: MeetingPhase,
): boolean {
  switch (control) {
    case "start":
      return phase === "idle";
    case "cancel":
      return phase === "starting";
    case "stop":
      return phase === "recording";
    case "reset":
      return phase === "done" || phase === "error";
  }
}

export function sourceWarning(
  sources: MeetingState["sources"],
): string | null {
  if (!sources.mic && !sources.system)
    return "No audio is reaching Tidy. Check the microphone and screen-recording permissions in System Settings, then restart the recording.";
  if (!sources.mic)
    return "The microphone is not being captured. Your own voice will be missing from this transcript.";
  if (!sources.system)
    return "System audio is not being captured. Other participants will be missing from this transcript.";
  return null;
}

/**
 * Why an automatic stop is counting down, as a short sentence: the call
 * ended, the room went quiet, or the calendar event is over.
 */
export function countdownCause(reason: string): string {
  const call = /^Stopped automatically when (.+) ended the call$/.exec(reason);
  if (call) return `${call[1]} ended the call.`;
  if (/after 3 minutes of silence$/.test(reason))
    return "No one has spoken for 3 minutes.";
  const event = /^Stopped automatically 5 minutes after (.+) ended$/.exec(reason);
  if (event) return `${event[1]} ended 5 minutes ago.`;
  return "This meeting looks finished.";
}
