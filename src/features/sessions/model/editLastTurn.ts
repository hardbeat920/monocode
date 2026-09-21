import type { Attachment, Block, HarnessId, Session } from "./session";

/** Harnesses that can rewind provider state before resending an edited prompt. */
export function harnessSupportsEditLastTurn(harness: HarnessId): boolean {
  return (
    harness === "pi" ||
    harness === "omp" ||
    harness === "codex" ||
    harness === "opencode"
  );
}

/** Index of the user block that starts the latest turn. */
export function lastUserTurnStartIndex(blocks: Block[]): number {
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (block.role === "user" && !block.internal && !block.draft) return index;
  }
  return -1;
}

export function lastUserTurnBlock(blocks: Block[]): Block | undefined {
  const index = lastUserTurnStartIndex(blocks);
  return index >= 0 ? blocks[index] : undefined;
}

export function truncateBeforeLastUserTurn(blocks: Block[]): Block[] {
  const start = lastUserTurnStartIndex(blocks);
  return start < 0 ? blocks : blocks.slice(0, start);
}

export type LastTurnRecall = {
  text: string;
  attachments: Attachment[];
};

export function lastTurnRecall(session: Session): LastTurnRecall | null {
  const block = lastUserTurnBlock(session.blocks);
  if (!block?.text.trim() && !block?.attachments?.length) return null;
  return {
    text: block.text,
    attachments: block.attachments ?? [],
  };
}

export function canEditLastTurn(session: Session): boolean {
  if (session.inboxAsk || session.busy || session.pendingQuestion) return false;
  if (session.editingQueuedMessageId) return false;
  if ((session.queuedMessages?.length ?? 0) > 0) return false;
  if (!harnessSupportsEditLastTurn(session.harness)) return false;
  const block = lastUserTurnBlock(session.blocks);
  if (!block || block.draft) return false;
  if (session.harness === "codex" && !block.providerTurnId) return false;
  if (block.secondOpinion || block.noteCard) return false;
  if (session.blocks.some((entry) => entry.role === "handoff")) return false;
  return true;
}
