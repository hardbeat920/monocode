import type { Block, Session } from "../../sessions/model/session";

/** Immutable acceptance provenance. Link JSON contains only this reference, never the task. */
export type AssignmentReference = {
  kind: "linked";
  parentId: string;
  childId: string;
  generation: number;
  requestKey: string;
  name?: string;
};
export type MessageReference = {
  kind: "message";
  parentId: string;
  childId: string;
  requestKey: string;
  name?: string;
};
export type AcceptedReference = AssignmentReference | MessageReference;
export type LinkedAssignment = AssignmentReference & { task: string };
export type AssignmentReceipt = AcceptedReference & {
  task: string;
  harness?: string;
  model?: string;
};

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}
const id = (value: unknown): value is string =>
  typeof value === "string" && /^[A-Za-z0-9_-]{1,512}$/.test(value);

export function sanitizeAcceptedReference(
  value: unknown,
): AcceptedReference | undefined {
  const raw = record(value);
  if (
    !raw ||
    !id(raw.parentId) ||
    !id(raw.childId) ||
    !id(raw.requestKey) ||
    (raw.name !== undefined &&
      (typeof raw.name !== "string" ||
        !raw.name.trim() ||
        raw.name.length > 80))
  )
    return undefined;
  const base = {
    parentId: raw.parentId,
    childId: raw.childId,
    requestKey: raw.requestKey,
    ...(typeof raw.name === "string" ? { name: raw.name } : {}),
  };
  if (raw.kind === "message" && raw.generation === undefined)
    return { kind: "message", ...base };
  if (
    raw.kind === "linked" &&
    Number.isSafeInteger(raw.generation) &&
    (raw.generation as number) > 0
  )
    return { kind: "linked", ...base, generation: raw.generation as number };
  return undefined;
}

export function sanitizeAssignmentReceipt(
  value: unknown,
): AssignmentReceipt | undefined {
  const raw = record(value);
  const reference = sanitizeAcceptedReference(value);
  if (!reference || typeof raw?.task !== "string" || raw.task.length > 240_000)
    return undefined;
  return {
    ...reference,
    task: raw.task,
    ...(typeof raw.harness === "string" && raw.harness.length <= 80
      ? { harness: raw.harness }
      : {}),
    ...(typeof raw.model === "string" && raw.model.length <= 512
      ? { model: raw.model }
      : {}),
  };
}

export function sameAssignment(
  a: AcceptedReference,
  b: AcceptedReference,
): boolean {
  return (
    a.kind === b.kind &&
    a.parentId === b.parentId &&
    a.childId === b.childId &&
    a.requestKey === b.requestKey &&
    (a.kind !== "linked" ||
      (b.kind === "linked" && a.generation === b.generation))
  );
}

export function assignmentTask(
  blocks: Block[],
  reference: AssignmentReference,
): string | undefined {
  return blocks.find(
    (block) =>
      block.role === "system" &&
      block.assignmentReceipt &&
      sameAssignment(block.assignmentReceipt, reference),
  )?.assignmentReceipt?.task;
}

/**
 * Stamp a renamed child's name on the parent's receipts and session updates
 * for it, so the rows keep the name after the link is gone. `sameAssignment`
 * ignores names, so acceptance identity is unchanged. Returns `blocks` itself
 * when nothing changes.
 */
export function renameLinkedAgent(
  blocks: Block[],
  parentId: string,
  childId: string,
  name: string,
): Block[] {
  const stale = (reference: { parentId: string; childId: string; name?: string } | undefined) =>
    !!reference && reference.parentId === parentId && reference.childId === childId && reference.name !== name;
  let changed = false;
  const next = blocks.map((block) => {
    if (stale(block.assignmentReceipt)) {
      changed = true;
      return { ...block, assignmentReceipt: { ...block.assignmentReceipt!, name } };
    }
    const updates = block.sessionUpdate?.updates;
    if (!updates?.some((update) => stale(update.assignment))) return block;
    changed = true;
    return {
      ...block,
      sessionUpdate: {
        ...block.sessionUpdate!,
        updates: updates.map((update) =>
          stale(update.assignment) ? { ...update, assignment: { ...update.assignment!, name } } : update,
        ),
      },
    };
  });
  return changed ? next : blocks;
}

export type AssignmentReceiptHost = {
  /** Undefined must mean a successful authoritative store lookup found no parent, not merely a closed tab. */
  session(id: string): Promise<Session | undefined>;
  /** Publish the immutable block before awaiting the queued store write. */
  replace(session: Session): Session;
  persist(session: Session): Promise<unknown>;
};

const repairs = new Map<string, Promise<void>>();
/** Replays and deletion use the very same evidence as initial acceptance. Never submits or links. */
export async function repairAssignmentReceipts(
  child: Session,
  host: AssignmentReceiptHost,
  options: {
    skipDeletedParents?: boolean;
    requestKey?: string;
    existingReceiptsDurable?: boolean;
  } = {},
): Promise<void> {
  for (const block of child.blocks) {
    const reference = block.acceptedAssignment;
    if (
      block.role !== "user" ||
      block.draft ||
      !reference ||
      block.appRequestId !== reference.requestKey ||
      reference.childId !== child.id ||
      (options.requestKey !== undefined &&
        reference.requestKey !== options.requestKey)
    )
      continue;
    const prior = repairs.get(reference.parentId) ?? Promise.resolve();
    const run = prior
      .catch(() => undefined)
      .then(async () => {
        const parent = await host.session(reference.parentId);
        if (!parent) {
          if (options.skipDeletedParents) return;
          throw new Error(
            `Cannot preserve accepted task: parent ${reference.parentId} is unavailable`,
          );
        }
        let next = parent;
        const exists = parent.blocks.some(
          (item) =>
            item.role === "system" &&
            item.assignmentReceipt &&
            sameAssignment(item.assignmentReceipt, reference),
        );
        if (exists && options.existingReceiptsDurable) return;
        if (!exists) {
          next = {
            ...parent,
            blocks: [
              ...parent.blocks,
              {
                id: crypto.randomUUID(),
                role: "system",
                text: "",
                assignmentReceipt: {
                  ...reference,
                  task: block.text,
                  ...(block.turnModel
                    ? {
                        harness: block.turnModel.harness,
                        model: block.turnModel.id,
                      }
                    : {}),
                },
              },
            ],
          };
          next = host.replace(next);
        }
        // Also retry a failed write when the receipt already exists in memory.
        if (!(await host.persist(next)))
          throw new Error("Parent assignment receipt was not saved");
      });
    repairs.set(reference.parentId, run);
    try {
      await run;
    } finally {
      if (repairs.get(reference.parentId) === run)
        repairs.delete(reference.parentId);
    }
  }
}

/** Acceptance is irreversible. A persistence problem is an accepted response, not a retry invitation. */
export async function persistAcceptedAssignment(
  child: Session,
  host: AssignmentReceiptHost,
  requestKey?: string,
): Promise<{ persistenceError?: string }> {
  try {
    if (!(await host.persist(child)))
      throw new Error("Accepted child evidence was not saved");
    await repairAssignmentReceipts(child, host, { requestKey });
    return {};
  } catch (error) {
    return {
      persistenceError: `Work was accepted; persistence needs repair: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
}
