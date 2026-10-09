import {
  automationScheduleLabel,
  isScheduleKind,
  nextAutomationRunAt,
  type AutomationScheduleKind,
} from "../../automations/model/automations";
import type { Block } from "../../sessions/model/session";
import type { MonoLook } from "./mono";
import {
  MonoFileConflict,
  monoContext,
  readAgentFile,
  writeAgentFile,
  type MonoFiles,
} from "./monoFiles";

/**
 * What a Mono does on its own, on a schedule. Each run happens in a hidden
 * session of its own, so the work never piles up in the Mono's conversation;
 * only what it decides to tell the user is posted to the chat, and a run with
 * nothing worth saying posts nothing at all.
 */
export type HabitSchedule = {
  scheduleKind: AutomationScheduleKind;
  /** Minute past the hour, for hourly habits. */
  minute: number;
  /** "HH:MM", local time. */
  time: string;
  /** 0 is Sunday, for weekly habits. */
  dayOfWeek: number;
};

export type Habit = {
  id: string;
  name: string;
  /** What to do and when it is worth telling the user. */
  instructions: string;
  schedule: HabitSchedule;
  enabled: boolean;
  createdAt: number;
  nextRunAt: number;
  lastRunAt?: number;
  /** "posted" told the user something; "quiet" found nothing worth saying. */
  lastOutcome?: "posted" | "quiet" | "failed";
  lastError?: string;
  /** Recent runs, newest first. The runs' sessions are not kept. */
  runs?: HabitRun[];
  /** Run once as soon as possible, paused or not, leaving the schedule alone. */
  runRequested?: boolean;
  /** The suggestion card in the chat it was started from. */
  fromCard?: string;
};

export type HabitOutcome = "posted" | "quiet" | "failed";

/** One run as the habit remembers it: when, how long, and what came of it. */
export type HabitRun = {
  at: number;
  durationMs: number;
  outcome: HabitOutcome;
  /** What it told the user, when it did. */
  report?: string;
  error?: string;
};

/** Runs kept per habit. */
export const HABIT_RUNS_KEPT = 20;
const REPORT_KEPT_CHARS = 4_000;

/** A run's whole reply when it has nothing worth telling the user. */
export const QUIET_MARKER = "NOTHING_TO_REPORT";
/** A run that was due longer ago than this is skipped, not caught up. */
export const MISSED_RUN_GRACE_MS = 2 * 60 * 60 * 1000;
/** Allow one hour of work per run; time waiting on the user is not counted. */
export const HABIT_RUN_TIMEOUT_MS = 60 * 60 * 1000;
/** An approval the user has not answered in this long is turned down. */
export const HABIT_APPROVAL_WAIT_MS = 12 * 60 * 60 * 1000;
export const HABITS_MAX = 20;

export function habitScheduleLabel(schedule: HabitSchedule): string {
  return automationScheduleLabel(schedule);
}

export function nextHabitRunAt(schedule: HabitSchedule, after: number): number {
  return nextAutomationRunAt(schedule, after);
}

function field<T>(
  value: Record<string, unknown>,
  key: string,
  check: (v: unknown) => v is T,
): T | undefined {
  return check(value[key]) ? value[key] : undefined;
}
const isString = (v: unknown): v is string => typeof v === "string";
const isNumber = (v: unknown): v is number =>
  typeof v === "number" && Number.isFinite(v);

/** habits.json as written by the app; anything malformed is left out. */
export function parseHabits(text: string | null | undefined): Habit[] {
  if (!text?.trim()) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const habits: Habit[] = [];
  for (const raw of parsed) {
    if (!raw || typeof raw !== "object") continue;
    const value = raw as Record<string, unknown>;
    const schedule = value.schedule as Record<string, unknown> | undefined;
    const id = field(value, "id", isString);
    const name = field(value, "name", isString);
    const instructions = field(value, "instructions", isString);
    const kind = schedule && field(schedule, "scheduleKind", isString);
    if (!id || !name || !instructions || !schedule || !kind) continue;
    if (!isScheduleKind(kind)) continue;
    const outcome = value.lastOutcome;
    habits.push({
      id,
      name,
      instructions,
      schedule: {
        scheduleKind: kind,
        minute: field(schedule, "minute", isNumber) ?? 0,
        time: field(schedule, "time", isString) ?? "09:00",
        dayOfWeek: field(schedule, "dayOfWeek", isNumber) ?? 1,
      },
      enabled: value.enabled !== false,
      createdAt: field(value, "createdAt", isNumber) ?? 0,
      nextRunAt: field(value, "nextRunAt", isNumber) ?? 0,
      ...(isNumber(value.lastRunAt) ? { lastRunAt: value.lastRunAt } : {}),
      ...(outcome === "posted" || outcome === "quiet" || outcome === "failed"
        ? { lastOutcome: outcome }
        : {}),
      ...(isString(value.lastError) ? { lastError: value.lastError } : {}),
      ...(Array.isArray(value.runs) ? { runs: parseRuns(value.runs) } : {}),
      ...(value.runRequested === true ? { runRequested: true } : {}),
      ...(isString(value.fromCard) ? { fromCard: value.fromCard } : {}),
    });
  }
  return habits;
}

function parseRuns(value: unknown[]): HabitRun[] {
  const runs: HabitRun[] = [];
  for (const raw of value) {
    if (!raw || typeof raw !== "object") continue;
    const run = raw as Record<string, unknown>;
    const outcome = run.outcome;
    if (
      !isNumber(run.at) ||
      (outcome !== "posted" && outcome !== "quiet" && outcome !== "failed")
    )
      continue;
    runs.push({
      at: run.at,
      durationMs: isNumber(run.durationMs) ? run.durationMs : 0,
      outcome,
      ...(isString(run.report) ? { report: run.report } : {}),
      ...(isString(run.error) ? { error: run.error } : {}),
    });
  }
  return runs.slice(0, HABIT_RUNS_KEPT);
}

export function serializeHabits(habits: Habit[]): string {
  return `${JSON.stringify(habits, null, 2)}\n`;
}

/** Validates a schedule from the CLI; times are local. */
export function habitSchedule(value: unknown): HabitSchedule {
  if (!value || typeof value !== "object")
    throw new Error(
      'schedule must be an object like {"kind":"weekdays","time":"09:00"}',
    );
  const input = value as Record<string, unknown>;
  const kind = input.kind;
  if (typeof kind !== "string" || !isScheduleKind(kind))
    throw new Error("schedule.kind must be hourly, daily, weekdays or weekly");
  const time = input.time ?? "09:00";
  if (typeof time !== "string" || !/^([01]\d|2[0-3]):[0-5]\d$/.test(time))
    throw new Error('schedule.time must be 24-hour "HH:MM"');
  const minute = input.minute ?? 0;
  if (
    !Number.isInteger(minute) ||
    (minute as number) < 0 ||
    (minute as number) > 59
  )
    throw new Error("schedule.minute must be 0-59");
  const dayOfWeek = input.dayOfWeek ?? 1;
  if (
    !Number.isInteger(dayOfWeek) ||
    (dayOfWeek as number) < 0 ||
    (dayOfWeek as number) > 6
  )
    throw new Error("schedule.dayOfWeek must be 0 (Sunday) to 6");
  const unknown = Object.keys(input).filter(
    (key) => !["kind", "time", "minute", "dayOfWeek"].includes(key),
  );
  if (unknown.length)
    throw new Error(`Unknown schedule fields: ${unknown.join(", ")}`);
  return {
    scheduleKind: kind,
    time,
    minute: minute as number,
    dayOfWeek: dayOfWeek as number,
  };
}

/** What it takes to start a habit, from the CLI or the panel. */
export type HabitDraft = Pick<Habit, "name" | "instructions" | "schedule">;

/** A new, enabled habit first due at its next time after `now`. */
export function newHabit(draft: HabitDraft, now: number): Habit {
  return {
    id: crypto.randomUUID(),
    ...draft,
    enabled: true,
    createdAt: now,
    nextRunAt: nextHabitRunAt(draft.schedule, now),
  };
}

/** Adds a habit to the Mono's list, up to HABITS_MAX. */
export function addHabit(monoId: string, draft: HabitDraft): Promise<Habit> {
  return updateHabits(monoId, (list) => {
    if (list.length >= HABITS_MAX)
      throw new Error(`A Mono can have at most ${HABITS_MAX} habits`);
    const created = newHabit(draft, Date.now());
    return { habits: [...list, created], result: created };
  });
}

/**
 * Habits to run now. One that came due while the app was closed or the
 * computer asleep runs once if it is recent; older ones just move on to
 * their next time, so waking the laptop does not set off a burst.
 */
export function dueHabits(
  habits: Habit[],
  now: number,
): { run: Habit[]; skipped: Habit[] } {
  const run: Habit[] = [];
  const skipped: Habit[] = [];
  for (const habit of habits) {
    // One run of a habit at a time; it comes due again once this one ends.
    if (active.has(habit.id)) continue;
    if (habit.runRequested) {
      run.push(habit);
      continue;
    }
    if (!habit.enabled || habit.nextRunAt > now) continue;
    (now - habit.nextRunAt <= MISSED_RUN_GRACE_MS ? run : skipped).push(habit);
  }
  return { run, skipped };
}

/** The habit as it stands after a run, or a skip, at `now`. */
export function afterRun(
  habit: Habit,
  now: number,
  outcome?: {
    kind: HabitOutcome;
    error?: string;
    report?: string;
    startedAt?: number;
  },
): Habit {
  const { runRequested, ...scheduled } = habit;
  const next: Habit = {
    ...scheduled,
    // A requested run leaves the schedule where it was.
    nextRunAt:
      runRequested && habit.nextRunAt > now
        ? habit.nextRunAt
        : nextHabitRunAt(habit.schedule, now),
  };
  if (!outcome) return next;
  const { lastError: _previous, ...rest } = next;
  const run: HabitRun = {
    at: outcome.startedAt ?? now,
    durationMs: Math.max(0, now - (outcome.startedAt ?? now)),
    outcome: outcome.kind,
    ...(outcome.report
      ? { report: outcome.report.slice(0, REPORT_KEPT_CHARS) }
      : {}),
    ...(outcome.error ? { error: outcome.error } : {}),
  };
  return {
    ...rest,
    lastRunAt: now,
    lastOutcome: outcome.kind,
    ...(outcome.error ? { lastError: outcome.error } : {}),
    runs: [run, ...(habit.runs ?? [])].slice(0, HABIT_RUNS_KEPT),
  };
}

/** Habits running right now, by id, with when each started. */
const active = new Map<string, number>();
const ACTIVE_CHANGED = "monocode:mono-habits-active";

export function setHabitRunning(habitId: string, startedAt?: number): void {
  if (startedAt == null) active.delete(habitId);
  else active.set(habitId, startedAt);
  window.dispatchEvent(new CustomEvent(ACTIVE_CHANGED));
}

export function habitRunningSince(habitId: string): number | undefined {
  return active.get(habitId);
}

const CHECK_NOW = "monocode:mono-habits-check-now";

/** Ask the scheduler to look for due habits now rather than at its next minute. */
export function checkHabitsNow(): void {
  window.dispatchEvent(new CustomEvent(CHECK_NOW));
}

export function subscribeCheckHabitsNow(onCheck: () => void): () => void {
  window.addEventListener(CHECK_NOW, onCheck);
  return () => window.removeEventListener(CHECK_NOW, onCheck);
}

/** Due and waiting for the scheduler to pick it up: Run now, or a missed minute. */
export function habitStarting(habit: Habit, now = Date.now()): boolean {
  return !!habit.runRequested || (habit.enabled && habit.nextRunAt <= now);
}

export function subscribeHabitsRunning(onChange: () => void): () => void {
  window.addEventListener(ACTIVE_CHANGED, onChange);
  return () => window.removeEventListener(ACTIVE_CHANGED, onChange);
}

/** The first message of a habit's hidden run. */
export function habitRunPrompt(
  look: MonoLook,
  files: MonoFiles,
  habit: Habit,
  now: number,
): string {
  const lastRun = habit.lastRunAt
    ? ` It last ran ${new Date(habit.lastRunAt).toLocaleString()}${
        habit.lastOutcome === "quiet"
          ? " and found nothing worth telling the user"
          : habit.lastOutcome === "failed"
            ? " and could not finish"
            : ""
      }.`
    : " This is its first run.";
  return `${monoContext(look, files, { soul: true, memory: true })}

<habit_run>
You are running one of your habits on your own; the user did not just message you, and they will not see this run. It is ${new Date(now).toLocaleString()}.${lastRun}

Habit: ${habit.name}
${habit.instructions.trim()}

Do the work now with your tools and the app CLI. Then reply with exactly the message you would send the user in your chat: short, in your own voice, only what they would want to know, with no preamble about running a habit. If there is nothing worth telling them, reply with exactly ${QUIET_MARKER} and nothing else; silence is the right answer more often than not. Save anything worth remembering with the memory actions before you reply.
</habit_run>`;
}

/** What to post, or nothing when the run chose to stay quiet. */
export function habitReport(text: string): string | undefined {
  const trimmed = text.trim();
  if (!trimmed) return undefined;
  if (trimmed.replace(/[`*.\s]/g, "") === QUIET_MARKER) return undefined;
  // A run that narrated before deciding to stay quiet ends on the marker.
  if (trimmed.endsWith(QUIET_MARKER) && trimmed.length < 400) return undefined;
  return trimmed.replace(new RegExp(`\\s*${QUIET_MARKER}\\s*$`), "").trim();
}

/** The habit posts that the Mono's own session has not seen yet. */
export function unseenHabitPosts(blocks: readonly Block[]): Block[] {
  const posts: Block[] = [];
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i];
    if (block.role === "user" && !block.draft && !block.internal) break;
    if (block.monoHabit && block.text.trim()) posts.unshift(block);
  }
  return posts;
}

/** Tells the Mono's session what its habits posted since the user last wrote. */
export function habitPostsContext(posts: readonly Block[]): string {
  if (!posts.length) return "";
  const lines = posts.map((post) => {
    const text = post.text.replace(/\s+/g, " ").trim();
    return `- ${new Date(post.monoHabit!.at).toLocaleString()} · ${post.monoHabit!.name}: ${
      text.length > 600 ? `${text.slice(0, 599)}…` : text
    }`;
  });
  return `<while_you_were_away>\nYour habits posted these to this chat on their own since the user's last message. The user has seen them; build on them rather than repeating them.\n${lines.join("\n")}\n</while_you_were_away>`;
}

/** The Mono's habits from its folder. */
export async function loadHabits(monoId: string): Promise<Habit[]> {
  return parseHabits((await readAgentFile(monoId, "habits.json")).text);
}

/** Claim one currently due habit; another window's claim forces a fresh check. */
export function claimHabit(
  monoId: string,
  habitId: string,
  now: number,
): Promise<Habit | undefined> {
  return updateHabits(monoId, (habits) => {
    const { run } = dueHabits(
      habits.filter((habit) => habit.id === habitId),
      now,
    );
    const claimed = run[0];
    return {
      habits: claimed
        ? habits.map((habit) =>
            habit.id === habitId ? afterRun(habit, now) : habit,
          )
        : habits,
      result: claimed,
    };
  });
}

/**
 * Change habits.json over the version that was read, retrying when the panel
 * or another run saved in between.
 */
export async function updateHabits<T>(
  monoId: string,
  change: (habits: Habit[]) => { habits: Habit[]; result: T },
): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    const file = await readAgentFile(monoId, "habits.json");
    const { habits, result } = change(parseHabits(file.text));
    try {
      await writeAgentFile(
        monoId,
        "habits.json",
        serializeHabits(habits),
        file.hash,
      );
      return result;
    } catch (error) {
      if (!(error instanceof MonoFileConflict) || attempt >= 2) throw error;
    }
  }
}

/**
 * Habit runs in flight, to the Mono each belongs to. Their sessions are
 * ephemeral, so this is all there is.
 */
const runs = new Map<string, string>();

export function markHabitRun(sessionId: string, monoId: string): void {
  runs.set(sessionId, monoId);
}

export function isHabitRun(sessionId: string): boolean {
  return runs.has(sessionId);
}

/** The Mono a habit run works for. */
export function habitRunMono(sessionId: string): string | undefined {
  return runs.get(sessionId);
}

export function clearHabitRun(sessionId: string): void {
  runs.delete(sessionId);
}

/**
 * An approval a hidden run asked for, put to the user in the Mono's chat
 * under an id of its own: the run's request ids would collide with the
 * Mono's. In memory only; a relay outlives neither its run nor the app.
 */
export type ApprovalRelay = {
  monoId: string;
  runId: string;
  requestId: number;
  blockId: string;
};

const relays = new Map<number, ApprovalRelay>();
let nextRelayId = 1_000_000_000;

export function relayApproval(relay: ApprovalRelay): number {
  const id = nextRelayId++;
  relays.set(id, relay);
  return id;
}

/** The relay behind a request in the Mono's chat, removed once answered. */
export function takeRelayedApproval(
  monoId: string,
  requestId: number,
): ApprovalRelay | undefined {
  const relay = relays.get(requestId);
  if (!relay || relay.monoId !== monoId) return undefined;
  relays.delete(requestId);
  return relay;
}

/** Whether a run's request has already been put to the user. */
export function isRelayed(runId: string, requestId: number): boolean {
  for (const relay of relays.values())
    if (relay.runId === runId && relay.requestId === requestId) return true;
  return false;
}

/** Relays still open when their run ended, to withdraw from the chat. */
export function dropRelays(runId: string): ApprovalRelay[] {
  const dropped: ApprovalRelay[] = [];
  for (const [id, relay] of relays)
    if (relay.runId === runId) {
      relays.delete(id);
      dropped.push(relay);
    }
  return dropped;
}

/** A habit's place in today: what it did, is doing, or is about to do. */
export type HabitToday = {
  habit: Habit;
  state: "running" | "starting" | "upcoming" | "done";
  /** When it started, starts, or ran. */
  at: number;
  /** How the run went, for one that already ran today. */
  outcome?: HabitOutcome;
};

function sameDay(a: number, b: number): boolean {
  return new Date(a).toDateString() === new Date(b).toDateString();
}

/**
 * Today at a glance, one row per habit: running or starting first, then the
 * day in order. A habit that ran and will run again today shows what is next;
 * one with nothing today is left out.
 */
export function habitsToday(
  habits: Habit[],
  now: number,
  runningSince: (habitId: string) => number | undefined = habitRunningSince,
): HabitToday[] {
  const items: HabitToday[] = [];
  for (const habit of habits) {
    const since = runningSince(habit.id);
    if (since != null) {
      items.push({ habit, state: "running", at: since });
      continue;
    }
    if (habitStarting(habit, now)) {
      items.push({ habit, state: "starting", at: now });
      continue;
    }
    if (
      habit.enabled &&
      habit.nextRunAt > now &&
      sameDay(habit.nextRunAt, now)
    ) {
      items.push({ habit, state: "upcoming", at: habit.nextRunAt });
      continue;
    }
    const last = habit.runs?.[0];
    const lastAt = last?.at ?? habit.lastRunAt;
    if (lastAt != null && sameDay(lastAt, now))
      items.push({
        habit,
        state: "done",
        at: lastAt,
        ...((last?.outcome ?? habit.lastOutcome)
          ? { outcome: last?.outcome ?? habit.lastOutcome }
          : {}),
      });
  }
  const rank = { running: 0, starting: 1, done: 2, upcoming: 2 } as const;
  return items.sort((a, b) => rank[a.state] - rank[b.state] || a.at - b.at);
}

/** The next run after today, for a day with nothing planned. */
export function nextHabitAfterToday(
  habits: Habit[],
  now: number,
): { habit: Habit; at: number } | undefined {
  let next: { habit: Habit; at: number } | undefined;
  for (const habit of habits)
    if (habit.enabled && (!next || habit.nextRunAt < next.at))
      next = { habit, at: habit.nextRunAt };
  return next && next.at > now ? next : undefined;
}
