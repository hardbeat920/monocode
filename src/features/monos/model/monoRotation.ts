import type {
  Block,
  ContextBoundaryMeta,
  Session,
} from "../../sessions/model/session";

/**
 * A Mono's chat never ends, but the provider session behind it does. Every
 * turn resends the whole native context, so once it grows past a budget, or
 * the user comes back after a long break, the next turn quietly starts a fresh
 * provider session. That session gets the soul and memory (see `monoFiles`)
 * plus a brief of where the conversation was: recent exchanges word for word
 * and a line for each earlier one. Nothing is summarized by a model, so a
 * rotation costs nothing and the brief never drifts into a summary of a
 * summary; the full transcript stays in the chat for the Mono to read.
 */

/**
 * What a fresh provider session costs before any conversation: the harness's
 * system prompt and tools, project instructions, the soul and memory. It is
 * measured on each fresh session (see `saveMonoBaseline`); this is the guess
 * until the first measurement.
 */
export const DEFAULT_BASELINE = 20_000;
/**
 * Rotate once the conversation has added this much on top of the baseline.
 * Cost is per token resent on every model call, whatever the window, so big
 * windows still rotate here rather than at a percentage of them.
 */
export const GROWTH_TOKENS = 60_000;
/** Or once it fills this much of a smaller window, before native compaction. */
export const ROTATE_RATIO = 0.5;
/**
 * A break this long starts fresh. Any prompt cache is long cold by then, so
 * continuing would pay to rewrite the whole context anyway...
 */
export const IDLE_ROTATE_MS = 6 * 60 * 60 * 1000;
/** ...as long as it has grown to at least this many baselines. */
export const IDLE_MIN_BASELINES = 2;

/** Recent exchanges carried word for word. */
const VERBATIM_EXCHANGES = 2;
const VERBATIM_USER_CHARS = 2_000;
const VERBATIM_REPLY_CHARS = 3_000;
/** Earlier exchanges carried as one line each, newest kept. */
const LINE_CHARS = 280;
const EARLIER_CHARS = 3_000;

export type RotationReason = "context" | "idle";

/** What the latest rotation started the provider session with. */
export type MonoRotation = {
  at: number;
  reason: RotationReason;
  /** The last block the replaced session saw; the fresh one takes it from here. */
  afterBlockId?: string;
  /** One line per exchange before the verbatim ones, oldest first. */
  earlier: string[];
};

type Exchange = {
  id: string;
  at?: number;
  user: string;
  reply: string;
  /** Posted by this habit on its own, with no user message before it. */
  habit?: string;
  completion?: string;
};

/** When the last turn finished, or started if it never reported a length. */
function lastActivity(blocks: readonly Block[]): number | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i];
    if (block.role !== "user" || block.draft || block.startedAt == null)
      continue;
    return block.startedAt + (block.durationMs ?? 0);
  }
  return undefined;
}

/** Whether the next turn should start a fresh provider session, and why. */
export function rotationReason(
  session: Pick<Session, "providerSessionId" | "context" | "blocks">,
  now: number,
  baseline = DEFAULT_BASELINE,
): RotationReason | undefined {
  // Nothing to rotate: the next turn already starts a new session.
  if (!session.providerSessionId) return undefined;
  const used = session.context?.used ?? 0;
  const window = session.context?.window;
  if (
    used >= baseline + GROWTH_TOKENS ||
    (window && used / window >= ROTATE_RATIO)
  )
    return "context";
  const last = lastActivity(session.blocks);
  if (
    last != null &&
    now - last >= IDLE_ROTATE_MS &&
    used >= baseline * IDLE_MIN_BASELINES
  )
    return "idle";
  return undefined;
}

/** The user's turns and what the Mono finally said in each. */
function exchanges(blocks: readonly Block[], afterId?: string): Exchange[] {
  const after = afterId
    ? blocks.findIndex((block) => block.id === afterId)
    : -1;
  const out: Exchange[] = [];
  for (const block of blocks.slice(after + 1)) {
    if (block.role === "user" && block.monoSessionCompletion) {
      out.push({
        id: block.id,
        at: block.startedAt,
        user: "",
        reply: "",
        completion: block.monoSessionCompletion.title,
      });
      continue;
    }
    if (block.internal || block.draft) continue;
    if (block.role === "user") {
      out.push({
        id: block.id,
        at: block.startedAt,
        user: block.text.trim(),
        reply: "",
      });
      continue;
    }
    if (block.monoHabit && block.text.trim()) {
      out.push({
        id: block.id,
        at: block.monoHabit.at,
        user: "",
        reply: block.text.trim(),
        habit: block.monoHabit.name,
      });
      continue;
    }
    const current = out[out.length - 1];
    if (
      current &&
      !current.habit &&
      block.role === "assistant" &&
      !block.tool &&
      !block.streaming &&
      block.text.trim()
    ) {
      // The last prose of a turn is its answer; earlier prose was narration.
      current.reply = block.text.trim();
    }
  }
  return out.filter((exchange) => exchange.user || exchange.reply);
}

function clip(text: string, max: number): string {
  if (text.length <= max) return text;
  const half = Math.floor((max - 20) / 2);
  return `${text.slice(0, half).trimEnd()} […] ${text.slice(-half).trimStart()}`;
}

function oneLine(text: string, max: number): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length <= max ? flat : `${flat.slice(0, max - 1).trimEnd()}…`;
}

const stamp = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

function when(at: number | undefined): string {
  return at == null ? "" : `${stamp.format(at)} · `;
}

function exchangeLine(exchange: Exchange): string {
  if (exchange.completion)
    return `- ${when(exchange.at)}You, reviewing session "${exchange.completion}": ${oneLine(exchange.reply, LINE_CHARS)}`;
  if (exchange.habit)
    return `- ${when(exchange.at)}You, from your habit "${exchange.habit}": ${oneLine(exchange.reply, LINE_CHARS)}`;
  return `- ${when(exchange.at)}User: ${oneLine(exchange.user, LINE_CHARS)}${
    exchange.reply ? ` → You: ${oneLine(exchange.reply, LINE_CHARS)}` : ""
  }`;
}

/** Keep the newest lines that fit, oldest first. */
function newestWithin(lines: string[], budget: number): string[] {
  const kept: string[] = [];
  let used = 0;
  for (let i = lines.length - 1; i >= 0; i--) {
    used += lines[i].length + 1;
    if (used > budget) break;
    kept.unshift(lines[i]);
  }
  return kept;
}

/**
 * Plan a rotation before the turn that triggers it, from the chat as it stands
 * (without that turn): what the fresh session is told about the conversation,
 * and the boundary that marks it in the chat.
 */
export function planRotation(
  blocks: readonly Block[],
  previous: MonoRotation | undefined,
  reason: RotationReason,
  now: number,
): {
  rotation: MonoRotation;
  brief: (sessionId: string) => string;
  boundary: ContextBoundaryMeta;
} {
  const segment = exchanges(blocks, previous?.afterBlockId);
  const verbatim = segment.slice(-VERBATIM_EXCHANGES);
  const lines = segment
    .slice(0, -VERBATIM_EXCHANGES || undefined)
    .map(exchangeLine);
  // Earlier lines are verbatim excerpts too, so carrying them is lossless
  // until they age out of the budget; the transcript keeps them for good.
  const earlier = newestWithin(
    [...(previous?.earlier ?? []), ...lines],
    EARLIER_CHARS,
  );
  const carried = [...earlier, ...verbatim.map(exchangeLine)];
  const rotation: MonoRotation = {
    at: now,
    reason,
    ...(blocks.length ? { afterBlockId: blocks[blocks.length - 1].id } : {}),
    // The next rotation sees these turns as earlier ones.
    earlier: newestWithin(carried, EARLIER_CHARS),
  };
  // The fresh session holds the newest exchanges word for word and a line for
  // each earlier one; the brief's instructions are for the agent, not the chat.
  const boundary: ContextBoundaryMeta = {
    kind: "rotation",
    trigger: "auto",
    reason,
    at: now,
    kept: verbatim.length ? "recent" : "none",
    ...(verbatim[0] ? { keptFromBlockId: verbatim[0].id } : {}),
    ...(earlier.length ? { summary: earlier.join("\n") } : {}),
  };
  const brief = (sessionId: string) => {
    const parts = [
      `This is the same long-lived conversation with the user. It continues in a fresh session so your context stays small; nothing was lost. Pick up where it left off and do not mention the switch unless asked. For anything older or more detailed than this, read the chat itself with \`app sessions.read {"sessionId":"${sessionId}"}\` (pass nextBefore for older pages), and check memory with \`app memory.search\`.`,
    ];
    if (earlier.length)
      parts.push(`Earlier, oldest first:\n${earlier.join("\n")}`);
    if (verbatim.length)
      parts.push(
        `Most recent, word for word:\n\n${verbatim
          .map((exchange) =>
            exchange.habit
              ? `You, on your own from your habit "${exchange.habit}" (${when(exchange.at).replace(/ · $/, "") || "earlier"}):\n${clip(exchange.reply, VERBATIM_REPLY_CHARS)}`
              : exchange.completion
                ? `You, reviewing session "${exchange.completion}" (${when(exchange.at).replace(/ · $/, "") || "earlier"}):\n${clip(exchange.reply, VERBATIM_REPLY_CHARS)}`
                : `User (${when(exchange.at).replace(/ · $/, "") || "earlier"}):\n${clip(exchange.user, VERBATIM_USER_CHARS)}${
                    exchange.reply
                      ? `\n\nYou:\n${clip(exchange.reply, VERBATIM_REPLY_CHARS)}`
                      : ""
                  }`,
          )
          .join("\n\n---\n\n")}`,
      );
    return `<previous_conversation>\n${parts.join("\n\n")}\n</previous_conversation>`;
  };
  return { rotation, brief, boundary };
}

const ROTATIONS_KEY = "monocode:mono-rotations";
const BASELINES_KEY = "monocode:mono-baselines";

type Baseline = { harness: string; tokens: number };

function readBaselines(): Record<string, Baseline> {
  try {
    const parsed = JSON.parse(localStorage.getItem(BASELINES_KEY) ?? "{}");
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

/** The measured baseline for this Mono on this harness, if there is one. */
export function loadMonoBaseline(
  sessionId: string,
  harness: string,
): number | undefined {
  const baseline = readBaselines()[sessionId];
  return baseline?.harness === harness && baseline.tokens > 0
    ? baseline.tokens
    : undefined;
}

/**
 * Record the first context reading of a fresh provider session. The first
 * request carries only the setup and the opening message, so it is the
 * closest reading to an empty session; the latest one wins, since the soul
 * and memory change over time.
 */
export function saveMonoBaseline(
  sessionId: string,
  harness: string,
  tokens: number,
): void {
  if (!Number.isFinite(tokens) || tokens <= 0) return;
  try {
    localStorage.setItem(
      BASELINES_KEY,
      JSON.stringify({
        ...readBaselines(),
        [sessionId]: { harness, tokens: Math.round(tokens) },
      }),
    );
  } catch {
    // Without it, rotation falls back to DEFAULT_BASELINE.
  }
}

function readRotations(): Record<string, MonoRotation> {
  try {
    const parsed = JSON.parse(localStorage.getItem(ROTATIONS_KEY) ?? "{}");
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

export function loadMonoRotation(sessionId: string): MonoRotation | undefined {
  return readRotations()[sessionId];
}

export function saveMonoRotation(
  sessionId: string,
  rotation: MonoRotation,
): void {
  try {
    localStorage.setItem(
      ROTATIONS_KEY,
      JSON.stringify({ ...readRotations(), [sessionId]: rotation }),
    );
  } catch {
    // Without the record the next rotation briefs from the whole chat.
  }
}

/** Delete the conversation brief and measurements along with its transcript. */
export function forgetMonoRotation(sessionId: string): void {
  const rotations = readRotations();
  const baselines = readBaselines();
  delete rotations[sessionId];
  delete baselines[sessionId];
  try {
    localStorage.setItem(ROTATIONS_KEY, JSON.stringify(rotations));
    localStorage.setItem(BASELINES_KEY, JSON.stringify(baselines));
  } catch {
    // A deleted conversation id is never reused.
  }
}
