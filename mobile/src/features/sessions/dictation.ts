import { getDictationPreference } from "@/features/settings/dictation";

import { dictationEngineInfo, ENGINES, engineFor, type Prepared } from "./dictationEngines";
import { planDictation, type DictationProblem, type EngineId } from "./dictationLanguages";

// Dictation is recorded and transcribed on the phone, by the engine and in
// the language picked in Settings → Dictation; only the words reach the
// desktop, so it works however the desktop is reached, or while it is away.

/** The phone's language ("en-US"): "Automatic" for an engine that can't
 *  detect the spoken language itself. */
export const dictationLocale = () =>
  Intl.DateTimeFormat().resolvedOptions().locale;

/** A dictation that can't go ahead, with what to tell the user. */
export class DictationError extends Error {
  constructor(readonly problem: DictationProblem) {
    super(problem.message);
    this.name = "DictationError";
  }
}

/** Whether any engine can run on this phone: iOS 26 or later for Apple's. */
export function canDictate() {
  return ENGINES.some((engine) => engine.canRun());
}

export type DictationSession = {
  engine: EngineId;
  prepared: Prepared;
  fellBackFrom?: EngineId;
};

/** Picks the engine and language and readies them: speech assets fetched if
 *  they aren't on the phone yet. Throws a DictationError when the engine or
 *  language can't be used. */
export async function prepareDictation(): Promise<DictationSession> {
  const plan = planDictation(
    getDictationPreference(),
    await dictationEngineInfo(),
    dictationLocale(),
  );
  if (!plan.ok) throw new DictationError(plan.problem);
  const prepared = await engineFor(plan.engine)!.prepare(plan.language, plan.variant);
  return { engine: plan.engine, prepared, fellBackFrom: plan.fellBackFrom };
}

/** The words in the recording at `uri`, or "" when there were none. */
export async function transcribe(session: DictationSession, uri: string) {
  return engineFor(session.engine)!.transcribe(uri, session.prepared);
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
