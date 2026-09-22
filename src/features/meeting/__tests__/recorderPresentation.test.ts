import { describe, expect, it } from "vitest";
import {
  controlEnabled,
  keepAudioNotice,
  sourceWarning,
} from "../recorderPresentation";
import {
  normaliseLanguage,
  readTranscriptionLanguage,
  writeTranscriptionLanguage,
  LANGUAGE_KEY,
} from "../transcriptionLanguage";

function memoryStore(initial: Record<string, string> = {}) {
  const values = { ...initial };
  return {
    values,
    getItem: (key: string) => values[key] ?? null,
    setItem: (key: string, value: string) => {
      values[key] = value;
    },
  };
}

describe("recorder presentation", () => {
  it("prefers the recording's own retention over the saved preference", () => {
    // Before a recording exists the preference is all we know.
    expect(keepAudioNotice("idle", false, true)).toBe(true);
    expect(keepAudioNotice("starting", false, true)).toBe(true);
    // Once the core has answered, this meeting's own answer wins, even if the
    // setting is changed while the meeting runs.
    expect(keepAudioNotice("recording", true, false)).toBe(true);
    expect(keepAudioNotice("transcribing", false, true)).toBe(false);
    expect(keepAudioNotice("done", true, false)).toBe(true);
    expect(keepAudioNotice("error", true, false)).toBe(true);
  });

  it("enables exactly one primary control per phase", () => {
    expect(controlEnabled("start", "idle")).toBe(true);
    expect(controlEnabled("start", "starting")).toBe(false);
    expect(controlEnabled("cancel", "starting")).toBe(true);
    expect(controlEnabled("cancel", "recording")).toBe(false);
    expect(controlEnabled("stop", "recording")).toBe(true);
    expect(controlEnabled("stop", "starting")).toBe(false);
    expect(controlEnabled("stop", "transcribing")).toBe(false);
    expect(controlEnabled("reset", "done")).toBe(true);
    expect(controlEnabled("reset", "error")).toBe(true);
    expect(controlEnabled("reset", "recording")).toBe(false);
  });

  it("warns about each missing capture source", () => {
    expect(sourceWarning({ mic: true, system: true })).toBeNull();
    expect(sourceWarning({ mic: false, system: true })).toContain(
      "microphone is not being captured",
    );
    expect(sourceWarning({ mic: true, system: false })).toContain(
      "System audio is not being captured",
    );
    expect(sourceWarning({ mic: false, system: false })).toContain(
      "No audio is reaching Tidy",
    );
  });
});

describe("transcription language", () => {
  it("falls back to English for anything unrecognised", () => {
    expect(normaliseLanguage("auto")).toBe("auto");
    expect(normaliseLanguage("en")).toBe("en");
    expect(normaliseLanguage(null)).toBe("en");
    expect(normaliseLanguage("klingon")).toBe("en");
  });

  it("remembers the last choice", () => {
    const store = memoryStore();
    expect(readTranscriptionLanguage(store)).toBe("en");
    writeTranscriptionLanguage("auto", store);
    expect(store.values[LANGUAGE_KEY]).toBe("auto");
    expect(readTranscriptionLanguage(store)).toBe("auto");
  });

  it("survives storage that refuses to answer", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readTranscriptionLanguage(broken)).toBe("en");
    expect(() => writeTranscriptionLanguage("auto", broken)).not.toThrow();
  });
});
