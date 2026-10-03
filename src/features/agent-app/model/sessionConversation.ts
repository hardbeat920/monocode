import type { Block, Session } from "../../sessions/model/session";
import { operatorUserPrompt } from "../../sessions/model/operatorCommand";

type Exchange = { user: Block; assistants: Block[] };

export type SessionReadOptions = {
  before?: string;
  limit?: number;
  maxChars?: number;
};

function capped(text: string, maxChars: number) {
  const trimmed = text.trim();
  return {
    text: trimmed.slice(0, maxChars),
    truncated: trimmed.length > maxChars,
  };
}

function messageCap(maxChars: number | undefined) {
  const cap = maxChars ?? 1200;
  if (!Number.isInteger(cap) || cap < 200 || cap > 6000)
    throw new Error("maxChars must be an integer from 200 to 6000");
  return cap;
}

function isReply(block: Block) {
  return block.role === "assistant" && !block.internal && !!block.text.trim();
}

function conversationExchanges(session: Session): Exchange[] {
  const exchanges: Exchange[] = [];
  for (const block of session.blocks) {
    if (block.role === "user" && !block.internal && !block.draft) {
      exchanges.push({ user: block, assistants: [] });
    } else if (isReply(block) && exchanges.length > 0) {
      exchanges[exchanges.length - 1].assistants.push(block);
    }
  }
  return exchanges;
}

function conversationTurn({ user, assistants }: Exchange, maxChars: number) {
  return {
    turnId: user.id,
    user: capped(operatorUserPrompt(user), maxChars),
    assistant: assistants.length
      ? capped(assistants[assistants.length - 1].text, maxChars)
      : null,
    earlierAssistantMessages: Math.max(0, assistants.length - 1),
  };
}

/** Show conversation prose only, with a stable cursor for older exchanges. */
export function sessionConversationPage(
  session: Session,
  options: SessionReadOptions = {},
) {
  const limit = options.limit ?? 3;
  if (!Number.isInteger(limit) || limit < 1 || limit > 3)
    throw new Error("limit must be an integer from 1 to 3");
  const maxChars = messageCap(options.maxChars);
  const exchanges = conversationExchanges(session);

  const end = options.before
    ? exchanges.findIndex((exchange) => exchange.user.id === options.before)
    : exchanges.length;
  if (end < 0) throw new Error("before is not a turn ID in this session");
  const start = Math.max(0, end - limit);
  const selected = exchanges.slice(start, end);
  return {
    sessionId: session.id,
    title: session.title,
    busy: !!session.busy,
    hasDraft: session.blocks.some(
      (block) => block.role === "user" && block.draft,
    ),
    turns: selected.map((exchange) => conversationTurn(exchange, maxChars)),
    nextBefore: start > 0 ? selected[0]?.user.id : null,
  };
}

/**
 * The run an app request submitted, including follow-ups steered into it, and
 * whether it ended. A run ends when the next submitted turn starts or the
 * session goes idle. Without a request ID, the newest exchange.
 */
export function sessionConversationTurn(
  session: Session,
  appRequestId?: string,
  maxChars?: number,
) {
  const cap = messageCap(maxChars);
  if (!appRequestId) {
    const exchanges = conversationExchanges(session);
    const latest = exchanges[exchanges.length - 1];
    return {
      turn: latest ? conversationTurn(latest, cap) : null,
      settled: !session.busy,
    };
  }
  const { blocks } = session;
  const start = blocks.findIndex(
    (block) =>
      block.role === "user" &&
      !block.draft &&
      block.appRequestId === appRequestId,
  );
  if (start < 0) return { turn: null, settled: false };
  const next = blocks.findIndex(
    (block, index) =>
      index > start &&
      block.role === "user" &&
      !block.draft &&
      block.startedAt != null,
  );
  const run = blocks.slice(start + 1, next < 0 ? undefined : next);
  return {
    turn: conversationTurn(
      { user: blocks[start], assistants: run.filter(isReply) },
      cap,
    ),
    settled: next >= 0 || !session.busy,
  };
}
