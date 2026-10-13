import { habitSchedule, type HabitSchedule } from "./monoHabits";

/**
 * Cards a Mono puts in its chat beside its words, with the app CLI's
 * `chat.card`. A card names what it is about and the app shows its current
 * state when it is drawn, so a PR card posted while the PR was open says
 * "Merged" the next day.
 */
export type MonoCard =
  | {
      type: "pr";
      /** owner/name; the project's own repository when left out. */
      repo?: string;
      number: number;
      /** One line from the Mono on why it matters. */
      note?: string;
    }
  | { type: "session"; sessionId: string; note?: string }
  | {
      /** Replies the user can send with a tap, after a question. */
      type: "choices";
      options: string[];
    }
  | {
      /** A habit the Mono suggests; nothing runs until the user starts it. */
      type: "habit";
      name: string;
      instructions: string;
      schedule: HabitSchedule;
    };

export const CARD_FIELDS = [
  "type",
  "repo",
  "number",
  "note",
  "sessionId",
  "options",
  "name",
  "instructions",
  "schedule",
] as const;

const MAX_CHOICES = 4;

function text(value: unknown, name: string, max: number): string {
  if (typeof value !== "string" || !value.trim() || value.length > max)
    throw new Error(`${name} must be text under ${max} characters`);
  return value.replace(/\s+/g, " ").trim();
}

function optionalNote(value: unknown) {
  return value === undefined ? {} : { note: text(value, "note", 280) };
}

function only(input: Record<string, unknown>, allowed: string[]) {
  const extra = Object.keys(input).filter(
    (key) => key !== "type" && !allowed.includes(key),
  );
  if (extra.length)
    throw new Error(
      `A ${String(input.type)} card does not take ${extra.join(", ")}`,
    );
}

/** A card from the CLI's input, checked field by field. */
export function parseCard(input: Record<string, unknown>): MonoCard {
  switch (input.type) {
    case "pr": {
      only(input, ["repo", "number", "note"]);
      const number = input.number;
      if (!Number.isInteger(number) || (number as number) < 1)
        throw new Error("number must be the PR's number, like 704");
      const repo =
        input.repo === undefined ? undefined : text(input.repo, "repo", 140);
      if (repo && !/^[\w.-]+\/[\w.-]+$/.test(repo))
        throw new Error('repo must look like "owner/name"');
      return {
        type: "pr",
        number: number as number,
        ...(repo ? { repo } : {}),
        ...optionalNote(input.note),
      };
    }
    case "session": {
      only(input, ["sessionId", "note"]);
      return {
        type: "session",
        sessionId: text(input.sessionId, "sessionId", 256),
        ...optionalNote(input.note),
      };
    }
    case "choices": {
      only(input, ["options"]);
      const options = input.options;
      if (
        !Array.isArray(options) ||
        options.length < 1 ||
        options.length > MAX_CHOICES
      )
        throw new Error(`options must be 1 to ${MAX_CHOICES} short replies`);
      return {
        type: "choices",
        options: options.map((option) => text(option, "each option", 80)),
      };
    }
    case "habit": {
      only(input, ["name", "instructions", "schedule"]);
      return {
        type: "habit",
        name: text(input.name, "name", 80),
        instructions: text(input.instructions, "instructions", 4_000),
        schedule: habitSchedule(input.schedule),
      };
    }
    default:
      throw new Error("type must be pr, session, choices or habit");
  }
}

/* Cards a habit's hidden run makes go out with its report, or not at all. */
const runCards = new Map<string, MonoCard[]>();

export function holdRunCard(runId: string, card: MonoCard): void {
  runCards.set(runId, [...(runCards.get(runId) ?? []), card]);
}

export function takeRunCards(runId: string): MonoCard[] {
  const cards = runCards.get(runId) ?? [];
  runCards.delete(runId);
  return cards;
}

/** What a session card shows about the session it points at. */
export type CardSession = {
  id: string;
  title: string;
  harness: string;
  busy: boolean;
  needsInput: boolean;
  /** The last thing its agent said, on one line. */
  lastLine?: string;
};

type SessionIndex = {
  sessions: Map<string, CardSession>;
  open?: (sessionId: string) => void;
};

const index: SessionIndex = { sessions: new Map() };
const INDEX_CHANGED = "monocode:mono-card-sessions";

/** The app keeps session cards current by publishing what they point at. */
export function publishCardSessions(
  sessions: CardSession[],
  open: (sessionId: string) => void,
): void {
  index.sessions = new Map(sessions.map((session) => [session.id, session]));
  index.open = open;
  window.dispatchEvent(new CustomEvent(INDEX_CHANGED));
}

export function cardSession(sessionId: string): CardSession | undefined {
  return index.sessions.get(sessionId);
}

export function openCardSession(sessionId: string): void {
  index.open?.(sessionId);
}

export function subscribeCardSessions(onChange: () => void): () => void {
  window.addEventListener(INDEX_CHANGED, onChange);
  return () => window.removeEventListener(INDEX_CHANGED, onChange);
}

const DECLINED_KEY = "monocode:mono-declined-habits";

/** Habit suggestions the user said no to, by the card's block id. */
export function declinedHabitCards(): Set<string> {
  try {
    const parsed = JSON.parse(localStorage.getItem(DECLINED_KEY) ?? "[]");
    return new Set(Array.isArray(parsed) ? parsed : []);
  } catch {
    return new Set();
  }
}

export function declineHabitCard(blockId: string): void {
  const declined = declinedHabitCards();
  declined.add(blockId);
  try {
    localStorage.setItem(DECLINED_KEY, JSON.stringify([...declined]));
  } catch {
    // It shows as undecided again after a restart; nothing else breaks.
  }
  window.dispatchEvent(new CustomEvent(INDEX_CHANGED));
}
