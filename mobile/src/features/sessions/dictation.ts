import { Platform } from "react-native";

// Dictation is transcribed on the phone, by Apple's SpeechTranscriber (iOS 26
// and up)

type AppleTranscription =
  typeof import("@react-native-ai/apple").AppleTranscription;

let transcription: AppleTranscription | null | undefined;

/** The native transcriber, loaded on first use: the package's turbo modules
 *  are looked up as it loads, and throw where they aren't built in. */
function transcriber() {
  if (transcription === undefined) {
    try {
      transcription =
        Platform.OS === "ios"
          ? // eslint-disable-next-line @typescript-eslint/no-require-imports
            (
              require("@react-native-ai/apple") as typeof import("@react-native-ai/apple")
            ).AppleTranscription
          : null;
    } catch {
      transcription = null;
    }
  }
  return transcription;
}

/** The phone's language, as the transcriber takes it ("en-US"). */
export const dictationLocale = () =>
  Intl.DateTimeFormat().resolvedOptions().locale;

/** Whether this phone can transcribe: iOS 26 or later, with the module built in. */
export function canDictate() {
  try {
    return transcriber()?.isAvailable(dictationLocale()) ?? false;
  } catch {
    return false;
  }
}

/** Fetches the language's speech assets if they aren't on the phone yet. */
export async function prepareDictation() {
  const native = transcriber();
  if (!native) throw new Error("Dictation isn't available on this device.");
  await native.prepare(dictationLocale());
}

/** The words in a recording, or "" when there were none. */
export async function transcribe(audio: ArrayBuffer) {
  const native = transcriber();
  if (!native) throw new Error("Dictation isn't available on this device.");
  const result = await native.transcribe(audio, dictationLocale());
  return result.segments
    .map((segment) => segment.text.trim())
    .filter(Boolean)
    .join(" ");
}

/** Below this the microphone hears only the room. */
const FLOOR_DB = -60;
const FLOOR_AMPLITUDE = 10 ** (FLOOR_DB / 20);

/** A meter reading in dB as a bar's height, 0 to 1: linear amplitude above
 *  the floor, square-rooted so speech fills the bars rather than peaking. */
export function meterLevel(db: number | undefined) {
  if (db === undefined || !Number.isFinite(db) || db <= FLOOR_DB) return 0;
  if (db >= 0) return 1;
  const amplitude = 10 ** (db / 20);
  return Math.sqrt((amplitude - FLOOR_AMPLITUDE) / (1 - FLOOR_AMPLITUDE));
}

/** "0:02", as the recording's running time. */
export function formatElapsed(millis: number) {
  const seconds = Math.floor(millis / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** Text with words put in at a selection, spaced from those either side. */
export function insertDictation(
  text: string,
  words: string,
  selection: { start: number; end: number },
) {
  const start = Math.min(selection.start, text.length);
  const end = Math.min(Math.max(selection.end, start), text.length);
  const before = text.slice(0, start);
  const after = text.slice(end);
  const lead = before && !/\s$/.test(before) ? " " : "";
  const trail = after && !/^\s/.test(after) ? " " : "";
  const inserted = `${lead}${words}${trail}`;
  return { text: before + inserted + after, cursor: start + inserted.length };
}
