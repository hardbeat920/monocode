import { unwrapShellCommand } from "../../../integrations/harness/core/shellIntent";
import { monoCodeToolCall } from "../../sessions/model/monocodeToolCall";
import {
  HARNESSES,
  sessionDraftBlock,
  sessionNeedsInput,
  type Block,
  type MonoSpawnedSession,
  type Session,
} from "../../sessions/model/session";

export function sanitizeMonoSpawnedSessions(
  value: unknown,
): MonoSpawnedSession[] {
  if (!Array.isArray(value)) return [];
  const result = new Map<string, MonoSpawnedSession>();
  for (const item of value) {
    if (
      !item ||
      typeof item !== "object" ||
      typeof item.sessionId !== "string" ||
      !/^[A-Za-z0-9_-]{1,512}$/.test(item.sessionId) ||
      typeof item.cwd !== "string" ||
      !item.cwd.trim() ||
      typeof item.title !== "string" ||
      !HARNESSES.includes(item.harness) ||
      typeof item.model !== "string" ||
      !item.model.trim()
    )
      continue;
    if (!result.has(item.sessionId))
      result.set(item.sessionId, {
        sessionId: item.sessionId,
        cwd: item.cwd,
        title: item.title.trim().slice(0, 300) || "Agent session",
        harness: item.harness,
        model: item.model,
      });
  }
  return [...result.values()];
}

/** Record accepted launches and follow-ups; retries never duplicate a card. */
export function recordMonoSpawnedSession(
  mono: Session,
  turnId: string,
  launched: MonoSpawnedSession,
): Session {
  const index = mono.blocks.findIndex(
    (block) => block.id === turnId && block.role === "user",
  );
  if (index < 0) return mono;
  const block = mono.blocks[index];
  if (
    block.monoSpawnedSessions?.some(
      (entry) => entry.sessionId === launched.sessionId,
    )
  )
    return mono;
  const blocks = mono.blocks.slice();
  blocks[index] = {
    ...block,
    monoSpawnedSessions: [...(block.monoSpawnedSessions ?? []), launched],
  };
  return { ...mono, blocks };
}

type SessionReceipt = {
  launches: MonoSpawnedSession[];
  listed: MonoSpawnedSession[];
  sentSessionId?: string;
};

const blockReceipts = new WeakMap<Block, SessionReceipt>();

/** Blocks are immutable, so old receipts only need to be parsed once. */
function sessionReceipt(block: Block): SessionReceipt {
  const cached = blockReceipts.get(block);
  if (cached) return cached;
  const found: SessionReceipt = {
    launches: sanitizeMonoSpawnedSessions(block.monoSpawnedSessions),
    listed: [],
  };
  if (
    block.tool &&
    ["completed", "success"].includes(block.tool.status ?? "")
  ) {
    const command =
      block.tool.preview?.kind === "shell"
        ? block.tool.preview.title
        : (block.tool.title ?? block.text);
    const call = command
      ? monoCodeToolCall({
          ...block,
          tool: {
            ...block.tool,
            preview: { kind: "shell", title: unwrapShellCommand(command) },
          },
        })
      : undefined;
    if (
      call &&
      ["sessions.start", "sessions.list", "sessions.send"].includes(call.action)
    ) {
      try {
        const receipt = JSON.parse(block.tool.detail ?? "");
        if (
          call.action === "sessions.start" &&
          receipt?.ok === true &&
          typeof receipt.result?.id === "string" &&
          receipt.result.id.startsWith("app-")
        ) {
          const entry = sanitizeMonoSpawnedSessions([
            {
              ...receipt.result,
              sessionId: receipt.result.id,
              title: receipt.result.title ?? "Agent session",
            },
          ])[0];
          if (
            entry &&
            !found.launches.some(
              (existing) => existing.sessionId === entry.sessionId,
            )
          )
            found.launches.push(entry);
        } else if (
          call.action === "sessions.list" &&
          receipt?.ok === true &&
          Array.isArray(receipt.result?.sessions)
        ) {
          found.listed = sanitizeMonoSpawnedSessions(
            receipt.result.sessions.map((session: Record<string, unknown>) => ({
              ...session,
              sessionId: session?.id,
              cwd: receipt.result.cwd,
            })),
          );
        } else if (
          call.action === "sessions.send" &&
          receipt?.ok === true &&
          receipt.result?.submitted === true &&
          typeof receipt.result.sessionId === "string"
        ) {
          found.sentSessionId = receipt.result.sessionId;
        }
      } catch {
        // Incomplete or truncated output is not a confirmed launch.
      }
    }
  }
  blockReceipts.set(block, found);
  return found;
}

/** Saved chats predate launch metadata, so recover successful CLI receipts too. */
export function monoSpawnedSessions(
  blocks: readonly Block[],
  knownSessions: readonly MonoSpawnedSession[] = [],
): MonoSpawnedSession[] {
  const known = new Map(knownSessions.map((entry) => [entry.sessionId, entry]));
  const receipts = blocks.map(sessionReceipt);
  // A listing provides the details for an older accepted follow-up, but
  // listing or reading a session alone never puts a card in the chat.
  for (const receipt of receipts) {
    for (const entry of [...receipt.listed, ...receipt.launches]) {
      known.set(entry.sessionId, entry);
    }
  }
  const found = new Map<string, MonoSpawnedSession>();
  for (const receipt of receipts) {
    for (const entry of receipt.launches) {
      if (!found.has(entry.sessionId)) found.set(entry.sessionId, entry);
    }
    const sent = receipt.sentSessionId && known.get(receipt.sentSessionId);
    if (sent && !found.has(sent.sessionId)) found.set(sent.sessionId, sent);
  }
  return [...found.values()];
}

/** Where a launched session stands, for its card in the Mono's chat. */
export type LaunchedSessionState =
  "working" | "input" | "draft" | "failed" | "stopped" | "done";

export type LaunchedSessionView = {
  state: LaunchedSessionState;
  title: string;
};

export function launchedSessionState(session: Session): LaunchedSessionState {
  if (sessionNeedsInput(session)) return "input";
  if (session.busy) return "working";
  if (sessionDraftBlock(session)) return "draft";
  const notice = session.blocks[session.blocks.length - 1]?.notice;
  if (notice === "error") return "failed";
  if (notice === "interrupt") return "stopped";
  return "done";
}
