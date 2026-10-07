import { unwrapShellCommand } from "../../../integrations/harness/core/shellIntent";
import { monoCodeToolCall } from "../../sessions/model/monocodeToolCall";
import {
  HARNESSES,
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

/** Record only accepted launches; retries never duplicate a card. */
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

const blockLaunches = new WeakMap<Block, MonoSpawnedSession[]>();

/** Blocks are immutable, so old receipts only need to be parsed once. */
function launchesInBlock(block: Block): MonoSpawnedSession[] {
  const cached = blockLaunches.get(block);
  if (cached) return cached;
  const found = sanitizeMonoSpawnedSessions(block.monoSpawnedSessions);
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
    if (call?.action === "sessions.start") {
      try {
        const receipt = JSON.parse(block.tool.detail ?? "");
        if (
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
            !found.some((existing) => existing.sessionId === entry.sessionId)
          )
            found.push(entry);
        }
      } catch {
        // Incomplete or truncated output is not a confirmed launch.
      }
    }
  }
  blockLaunches.set(block, found);
  return found;
}

/** Saved chats predate launch metadata, so recover successful CLI receipts too. */
export function monoSpawnedSessions(
  blocks: readonly Block[],
): MonoSpawnedSession[] {
  const found = new Map<string, MonoSpawnedSession>();
  for (const block of blocks) {
    for (const entry of launchesInBlock(block)) {
      if (!found.has(entry.sessionId)) found.set(entry.sessionId, entry);
    }
  }
  return [...found.values()];
}
