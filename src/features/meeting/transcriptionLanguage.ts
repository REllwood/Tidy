/**
 * The spoken language passed to transcription. Kept in localStorage rather than
 * the UI store so the recorder remembers the last choice without persisting any
 * meeting content.
 */
export type TranscriptionLanguage = "en" | "auto";

export const LANGUAGE_KEY = "tidy.transcription-language";

export const languageLabels: Record<TranscriptionLanguage, string> = {
  en: "English",
  auto: "Detect automatically",
};

/** Anything unrecognised (including a cleared or corrupted value) falls back to English. */
export function normaliseLanguage(raw: string | null): TranscriptionLanguage {
  return raw === "auto" ? "auto" : "en";
}

type Store = Pick<Storage, "getItem" | "setItem">;

function storage(): Store | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export function readTranscriptionLanguage(
  store: Store | null = storage(),
): TranscriptionLanguage {
  try {
    return normaliseLanguage(store?.getItem(LANGUAGE_KEY) ?? null);
  } catch {
    return "en";
  }
}

export function writeTranscriptionLanguage(
  value: TranscriptionLanguage,
  store: Store | null = storage(),
): void {
  try {
    store?.setItem(LANGUAGE_KEY, value);
  } catch {
    /* Private browsing or a full quota: the choice simply is not remembered. */
  }
}
