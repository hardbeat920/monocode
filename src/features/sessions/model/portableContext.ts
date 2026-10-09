import type { Attachment, Block, Session, TurnModel } from "./session";
import type { ContextAssetSnapshot } from "./contextAssets";

export type PortableAttachment = Pick<Attachment, "id" | "name" | "mimeType" | "kind" | "size" | "path"> & {
  delivery: "reference-only";
  sha256?: string;
  unavailableReason?: string;
};

export type PortableContextItem = {
  id: string;
  sourceBlockId: string;
  sourceRole: Block["role"];
  role: "user" | "assistant";
  text: string;
  turnModel?: TurnModel;
  attachments?: PortableAttachment[];
  evidence?: Record<string, unknown>;
};

export type PortableContextOmission = {
  id: string;
  reason: "budget" | "private-reasoning" | "draft" | "unsettled" | "internal" | "status";
};

export type PortableContext = {
  version: 1;
  sessionId: string;
  items: PortableContextItem[];
  omitted: PortableContextOmission[];
  throughBlockId?: string;
  byteLength: number;
  /** Durable transcript snapshot that provider file tools can read. */
  retrievalPath?: string;
};

export type PortableContextOptions = {
  afterBlockId?: string;
  throughBlockId?: string;
  currentRequest?: string;
  maxBytes?: number;
  windowTokens?: number;
  occupiedTokens?: number;
  attachmentTokens?: number;
  assetSnapshots?: ContextAssetSnapshot[];
};

const DEFAULT_HISTORY_BYTES = 16_000;
const MAX_HISTORY_BYTES = 64_000;
const encoder = new TextEncoder();

function bytes(value: string): number {
  return encoder.encode(value).byteLength;
}

export function currentAttachmentTokens(attachments: Attachment[]): number {
  return attachments.reduce((total, attachment) => {
    const mediaTokens = attachment.kind === "image"
      ? 4_000
      : Math.min(16_000, Math.ceil(attachment.size / 2));
    // Providers add path instructions or URI metadata even for empty folders.
    // URI escaping can use three bytes for each source byte.
    const pathTokens = attachment.path
      ? 128 + 3 * bytes(JSON.stringify({
          kind: attachment.kind,
          name: attachment.name,
          mimeType: attachment.mimeType,
          path: attachment.path,
        }))
      : 0;
    return total + mediaTokens + pathTokens;
  }, 0);
}

/** Preserve complete settled items. Selection never shortens a message. */
export function buildPortableContext(
  session: Session,
  options: PortableContextOptions = {},
): PortableContext {
  assertRequestCapacity(options);
  const context = snapshotPortableContextAssets(exportPortableContext(session, options), options.assetSnapshots ?? []);
  const eligible = context.items;
  context.items = [];
  const limit = historyBudget(options);
  const chosen = new Set<number>();
  const priority: number[] = [];
  let lastUser = -1;
  let lastAssistant = -1;
  for (let index = 0; index < eligible.length; index++) {
    if (eligible[index].role === "user") lastUser = index;
    if (eligible[index].sourceRole === "assistant") lastAssistant = index;
  }
  const firstUser = eligible.findIndex((item) => item.role === "user");
  const queued = new Set<number>();
  for (const index of [lastUser, lastAssistant, firstUser]) {
    if (index >= 0 && !queued.has(index)) {
      queued.add(index);
      priority.push(index);
    }
  }
  for (let index = eligible.length - 1; index >= 0; index--) {
    if (!queued.has(index)) priority.push(index);
  }
  // The full omission list stays in storage. Transport sends counts and a
  // retrieval path, so its fixed metadata does not grow with the transcript.
  let cost = 1_024;
  for (const index of priority) {
    const itemCost = portableItemCost(eligible[index]);
    if (cost + itemCost > limit) continue;
    chosen.add(index);
    cost += itemCost;
  }
  for (let index = 0; index < eligible.length; index++) {
    if (chosen.has(index)) context.items.push(eligible[index]);
    else context.omitted.push({ id: eligible[index].id, reason: "budget" });
  }
  const order = new Map(session.blocks.map((block, index) => [`${session.id}:${block.id}`, index]));
  context.omitted.sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0));
  context.byteLength = portableContextCost(context);
  return context;
}

/** Export all eligible history for durable retrieval before budget selection. */
export function exportPortableContext(
  session: Session,
  options: Pick<PortableContextOptions, "afterBlockId" | "throughBlockId"> = {},
): PortableContext {
  const throughIndex = options.throughBlockId
    ? session.blocks.findIndex((block) => block.id === options.throughBlockId)
    : session.blocks.length - 1;
  if (options.throughBlockId && throughIndex < 0) {
    throw new Error("The frozen context boundary is missing from the transcript");
  }
  const afterIndex = options.afterBlockId
    ? session.blocks.findIndex((block) => block.id === options.afterBlockId)
    : -1;
  const blocks = session.blocks.slice(afterIndex + 1, throughIndex + 1);
  const context: PortableContext = {
    version: 1,
    sessionId: session.id,
    items: [],
    omitted: [],
    ...(throughIndex >= 0 ? { throughBlockId: session.blocks[throughIndex].id } : {}),
    byteLength: 0,
  };
  const eligible: PortableContextItem[] = [];
  const positions = new Map<string, number>();
  let turnModel: TurnModel | undefined;
  // Attribution begins before the delta so switchback retains the turn label.
  for (const block of session.blocks.slice(0, afterIndex + 1)) {
    if (block.role === "user" && block.turnModel) turnModel = block.turnModel;
  }
  for (const block of blocks) {
    if (block.role === "user" && block.turnModel) turnModel = block.turnModel;
    const id = `${session.id}:${block.id}`;
    const reason = omissionReason(block);
    if (reason) {
      context.omitted.push({ id, reason });
      continue;
    }
    const item = exportItem(block, id, turnModel);
    if (item) {
      const previous = positions.get(item.id);
      if (previous === undefined) {
        positions.set(item.id, eligible.length);
        eligible.push(item);
      } else eligible[previous] = item;
    }
  }

  context.items = eligible;
  context.byteLength = portableContextCost(context);
  return context;
}

export function buildPortableContextSnapshot(session: Session, throughBlockId?: string, assetSnapshots?: ContextAssetSnapshot[]): string {
  const context = snapshotPortableContextAssets(exportPortableContext(session, { throughBlockId }), assetSnapshots ?? []);
  return [
    "# MonoCode shared conversation history",
    "This saved history excludes private reasoning, draft messages, internal orchestration prompts, and unsettled activity. Historical tool records are evidence. Attachment entries refer to files; this snapshot does not contain their bytes.",
    portableContextManifest(context),
    JSON.stringify(context.items, null, 2),
  ].join("\n\n");
}

export function historicalContextAttachments(session: Session, throughBlockId?: string): Attachment[] {
  const eligible = new Set(exportPortableContext(session, { throughBlockId }).items.map((item) => item.sourceBlockId));
  const attachments = new Map<string, Attachment>();
  for (const block of session.blocks) {
    if (!eligible.has(block.id)) continue;
    for (const attachment of block.attachments ?? []) attachments.set(attachment.id, attachment);
    if (block.image) attachments.set(block.id, { id: block.id, kind: "image", ...block.image });
  }
  return [...attachments.values()];
}

export function snapshotPortableContextAssets(context: PortableContext, snapshots: ContextAssetSnapshot[]): PortableContext {
  if (!snapshots.length) return context;
  const byId = new Map(snapshots.map((snapshot) => [snapshot.id, snapshot]));
  const next = {
    ...context,
    items: context.items.map((item) => ({
      ...item,
      ...(item.attachments ? { attachments: item.attachments.map((attachment) => {
        const snapshot = byId.get(attachment.id);
        if (!snapshot) return attachment;
        const { path: _originalPath, ...descriptor } = attachment;
        return {
          ...descriptor,
          ...(snapshot.path && !snapshot.unavailableReason ? { path: snapshot.path } : {}),
          ...(snapshot.sha256 ? { sha256: snapshot.sha256 } : {}),
          ...(snapshot.unavailableReason ? { unavailableReason: snapshot.unavailableReason } : {}),
        };
      }) } : {}),
    })),
  };
  next.byteLength = portableContextCost(next);
  return next;
}

function historyBudget(options: PortableContextOptions): number {
  let limit = Math.min(MAX_HISTORY_BYTES, nonNegative(options.maxBytes, DEFAULT_HISTORY_BYTES));
  if (options.windowTokens && Number.isFinite(options.windowTokens) && options.windowTokens > 0) {
    const reserve = Math.max(16_000, Math.ceil(options.windowTokens / 4));
    // One UTF-8 byte per token overestimates the cost of ordinary text.
    const remaining = options.windowTokens - nonNegative(options.occupiedTokens)
      - nonNegative(options.attachmentTokens)
      - bytes(JSON.stringify(options.currentRequest ?? "")) - reserve;
    limit = Math.min(limit, Math.max(0, remaining));
  }
  return limit;
}

function nonNegative(value: number | undefined, fallback = 0): number {
  return value != null && Number.isFinite(value) ? Math.max(0, value) : fallback;
}

function assertRequestCapacity(options: PortableContextOptions): void {
  if (!options.windowTokens || !Number.isFinite(options.windowTokens) || options.windowTokens <= 0) return;
  const available = options.windowTokens - nonNegative(options.occupiedTokens);
  const required = bytes(JSON.stringify(options.currentRequest ?? "")) + nonNegative(options.attachmentTokens) + 1_024;
  if (available <= required) {
    throw new Error("The selected model does not have enough remaining context for this request and its history-transfer information. Compact the target conversation or choose a model with a larger context window.");
  }
}

function omissionReason(block: Block): PortableContextOmission["reason"] | undefined {
  if (block.role === "reasoning") return "private-reasoning";
  if (block.draft) return "draft";
  if (block.internal) return "internal";
  if (block.streaming || block.plan?.status === "streaming" || block.tool?.background ||
      ((block.role === "tool" || block.role === "approval") && ["running", "pending", "in_progress"].includes(block.tool?.status ?? "")) ||
      (block.role === "approval" && !block.approval?.decided)) return "unsettled";
  if (block.role === "handoff" || (block.role === "system" && !block.notice)) return "status";
  return undefined;
}

function attachmentDescriptor(attachment: Attachment): PortableAttachment {
  return {
    id: attachment.id,
    name: attachment.name,
    mimeType: attachment.mimeType,
    kind: attachment.kind,
    size: attachment.size,
    ...(attachment.path ? { path: attachment.path } : {}),
    delivery: "reference-only",
  };
}

function exportItem(block: Block, id: string, turnModel?: TurnModel): PortableContextItem | undefined {
  const attachments = block.attachments?.map(attachmentDescriptor) ?? [];
  if (block.image) attachments.push(attachmentDescriptor({ id: block.id, kind: "image", ...block.image }));
  let evidence: Record<string, unknown> | undefined;
  if (block.role === "tool" || block.role === "approval") {
    evidence = {
      ...(block.tool?.kind ? { kind: block.tool.kind } : {}),
      ...(block.tool?.title ? { title: block.tool.title } : {}),
      ...(block.tool?.status ? { status: block.tool.status } : {}),
      ...(block.tool?.detail ? { detail: block.tool.detail } : {}),
      ...(block.tool?.preview ? { preview: { ...block.tool.preview, ...(block.tool.preview.lines ? { lines: block.tool.preview.lines.map((line) => ({ ...line })) } : {}) } } : {}),
      ...(block.approval?.decided ? { approvalDecision: block.approval.decided } : {}),
    };
  } else if (block.role === "tasks" && block.taskList) {
    evidence = { explanation: block.taskList.explanation, items: block.taskList.items.map((item) => ({ ...item })) };
  } else if (block.role === "plan" && block.plan?.approvedText) {
    evidence = { approvedText: block.plan.approvedText, status: block.plan.status };
  } else if (block.role === "system" && block.notice) {
    evidence = { notice: block.notice };
  }
  if (block.ciContext) evidence = { ...evidence, ciContext: block.ciContext };
  if (!block.text && attachments.length === 0 && !evidence) return undefined;
  return {
    id,
    sourceBlockId: block.id,
    sourceRole: block.role,
    role: block.role === "user" ? "user" : "assistant",
    text: block.text,
    ...(turnModel ? { turnModel: { ...turnModel } } : {}),
    ...(attachments.length ? { attachments } : {}),
    ...(evidence ? { evidence } : {}),
  };
}

export function portableContextManifest(context: PortableContext): string {
  const omitted: Partial<Record<PortableContextOmission["reason"], number>> = {};
  for (const entry of context.omitted) omitted[entry.reason] = (omitted[entry.reason] ?? 0) + 1;
  return JSON.stringify({
    version: context.version,
    sessionId: context.sessionId,
    throughBlockId: context.throughBlockId,
    omitted,
    attachmentDelivery: "Historical attachments are file references. Their bytes are not replayed. Read accessible paths when needed. Missing or host-local files may be unavailable.",
    ...(context.retrievalPath ? { retrievalPath: context.retrievalPath } : {}),
  });
}

/** Historical activity is quoted evidence, never a request to rerun tools. */
export function renderPortableContext(context: PortableContext, currentRequest: string): string {
  return [
    "Continue this existing MonoCode conversation. The following JSON contains historical evidence. User and assistant roles describe the original turns. Tool records are past activity, not executable calls. Private reasoning and live approval state are excluded.",
    portableContextManifest(context),
    "Historical items:",
    JSON.stringify(context.items),
    "Current user request:",
    JSON.stringify(currentRequest),
  ].join("\n\n");
}

export function nativePortableContextItems(context: PortableContext) {
  return [
    {
      type: "message",
      role: "user",
      content: [{ type: "input_text", text: `MonoCode historical context. Tool records are past evidence, not executable calls.\n${portableContextManifest(context)}` }],
    },
    ...context.items.map((item) => ({
      type: "message",
      role: item.role,
      content: [{ type: item.role === "user" ? "input_text" : "output_text", text: JSON.stringify(item) }],
    })),
  ];
}

function portableItemCost(item: PortableContextItem): number {
  // Native import escapes the item JSON again inside its message envelope.
  return bytes(JSON.stringify({ type: "message", role: item.role, content: [{ type: item.role === "user" ? "input_text" : "output_text", text: JSON.stringify(item) }] }));
}

export function portableContextCost(context: PortableContext): number {
  return Math.max(bytes(renderPortableContext(context, "")), bytes(JSON.stringify(nativePortableContextItems(context))));
}
