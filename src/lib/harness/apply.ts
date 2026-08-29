import type { Attachment, Block, Session, ToolPreview } from "../session";
import { mergeContextUsage } from "../contextUsage";
import { displayPath } from "../paths";
import {
  composeToolTitle,
  isFileTool,
  isWeakToolTitle,
  mergeToolPreview,
  stubFilePreview,
} from "./preview";
import { joinStreamText } from "./streamText";
import type { HarnessEvent } from "./types";

export function applyHarnessEvent(
  session: Session,
  event: HarnessEvent,
): Session {
  switch (event.type) {
    case "message.delta":
      return patchStreaming(session, "assistant", event.text, true);
    case "message.completed":
      return finishRole(session, "assistant");
    case "reasoning.delta":
      return patchStreaming(session, "reasoning", event.text, true);
    case "reasoning.completed":
      return finishRole(session, "reasoning");
    case "tool.started":
      return upsertTool(session, {
        callId: event.callId,
        title: event.title,
        kind: event.kind,
        status: event.status,
        preview: event.preview,
        streaming: true,
      });
    case "tool.updated":
      return upsertTool(session, {
        callId: event.callId,
        title: event.title,
        kind: event.kind,
        status: event.status,
        detail: event.detail,
        preview: event.preview,
        streaming: event.status !== "completed" && event.status !== "failed",
      });
    case "approval.requested":
      return attachApproval(session, event);
    case "approval.resolved": {
      const blocks = session.blocks.map((block) =>
        block.approval?.requestId === event.requestId
          ? {
              ...block,
              approval: { ...block.approval, decided: event.decision },
            }
          : block,
      );
      return { ...session, blocks };
    }
    case "context":
      return {
        ...session,
        context: mergeContextUsage(session.context, {
          used: event.used,
          window: event.window,
        }),
      };
    case "plan":
      return appendBlock(session, {
        id: crypto.randomUUID(),
        role: "plan",
        text: event.text,
      });
    case "session.error":
      return appendBlock(stopStreaming(session), {
        id: crypto.randomUUID(),
        role: "system",
        text: event.message,
      });
    case "session.providerBound":
      return { ...session, providerSessionId: event.providerSessionId };
    case "status":
      return appendStatus(session, event.text);
    default:
      return session;
  }
}

export function appendUser(
  session: Session,
  text: string,
  attachments: Attachment[] = [],
  extra?: { secondOpinion?: Block["secondOpinion"] },
): Session {
  return appendBlock(
    { ...session, busy: true },
    {
      id: crypto.randomUUID(),
      role: "user",
      text,
      startedAt: Date.now(),
      ...(attachments.length > 0 ? { attachments } : {}),
      ...(extra?.secondOpinion ? { secondOpinion: extra.secondOpinion } : {}),
    },
  );
}

/** Append a follow-up user message during an active turn without sealing streams. */
export function appendSteerUser(
  session: Session,
  text: string,
  attachments: Attachment[] = [],
): Session {
  return {
    ...session,
    busy: true,
    blocks: [
      ...session.blocks,
      {
        id: crypto.randomUUID(),
        role: "user",
        text,
        ...(attachments.length > 0 ? { attachments } : {}),
      },
    ],
  };
}

export function stopStreaming(session: Session): Session {
  return {
    ...session,
    busy: false,
    blocks: stampTurnDuration(
      session.blocks.map((block) =>
        block.streaming ? { ...block, streaming: false } : block,
      ),
    ),
  };
}

function stampTurnDuration(blocks: Block[]): Block[] {
  let lastUser = -1;
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].role === "user") {
      lastUser = i;
      break;
    }
  }
  if (lastUser < 0) return blocks;
  const user = blocks[lastUser];
  if (user.durationMs != null || user.startedAt == null) return blocks;
  const next = blocks.slice();
  next[lastUser] = {
    ...user,
    durationMs: Math.max(0, Date.now() - user.startedAt),
  };
  return next;
}

/** Status pings repeat; keep one row per run instead of stacking identical lines. */
function appendStatus(session: Session, text: string): Session {
  const trimmed = text.trim();
  if (!trimmed) return session;
  const last = [...session.blocks]
    .reverse()
    .find((block) => block.role !== "reasoning");
  if (last?.role === "system" && last.text === trimmed) return session;
  return appendBlock(session, {
    id: crypto.randomUUID(),
    role: "system",
    text: trimmed,
  });
}

function appendBlock(session: Session, block: Block): Session {
  return { ...session, blocks: [...sealLastStream(session.blocks), block] };
}

/** Append to the latest block only when it is the same role; never splice into an earlier one. */
function patchStreaming(
  session: Session,
  role: "assistant" | "reasoning",
  text: string,
  streaming: boolean,
): Session {
  if (!text && role === "reasoning") return session;
  const last = session.blocks[session.blocks.length - 1];
  if (last?.role === role) {
    const nextText = joinStreamText(last.text, text);
    if (nextText === last.text && last.streaming === streaming) return session;
    const blocks = session.blocks.slice();
    blocks[blocks.length - 1] = {
      ...last,
      text: nextText,
      streaming,
    };
    return { ...session, blocks };
  }
  const blocks = sealLastStream(session.blocks);
  blocks.push({
    id: crypto.randomUUID(),
    role,
    text,
    streaming,
  });
  return { ...session, blocks };
}

function attachApproval(
  session: Session,
  event: Extract<HarnessEvent, { type: "approval.requested" }>,
): Session {
  const index = findToolForApproval(session, event);
  if (index >= 0) {
    const blocks = session.blocks.slice();
    const prev = blocks[index];
    const preview = mergeToolPreview(event.preview, prev.tool?.preview);
    const label =
      finalToolLabel(
        session,
        event.kind ?? prev.tool?.kind,
        preferLabel(event.title, prev.tool?.title, prev.text),
        preview,
      ) || prev.text;
    blocks[index] = {
      ...prev,
      text: label || prev.text,
      tool: prev.tool
        ? {
            ...prev.tool,
            kind: event.kind ?? prev.tool.kind,
            title: label || prev.tool.title,
            ...(preview ? { preview } : {}),
          }
        : event.callId
          ? {
              callId: event.callId,
              title: label,
              kind: event.kind,
              ...(preview ? { preview } : {}),
            }
          : prev.tool,
      approval: { requestId: event.requestId },
    };
    return { ...session, blocks };
  }
  const preview = event.preview;
  const label =
    finalToolLabel(session, event.kind, preferLabel(event.title), preview) ||
    kindTitle(event.kind);
  return appendBlock(session, {
    id: crypto.randomUUID(),
    role: "tool",
    text: label,
    tool: {
      ...(event.callId ? { callId: event.callId } : {}),
      title: label,
      kind: event.kind,
      ...(preview ? { preview } : {}),
    },
    approval: { requestId: event.requestId },
  });
}

function findToolForApproval(
  session: Session,
  event: Extract<HarnessEvent, { type: "approval.requested" }>,
): number {
  if (event.callId) {
    const byId = session.blocks.findIndex(
      (block) => block.tool?.callId === event.callId,
    );
    if (byId >= 0) return byId;
  }
  const needle = normalizeLabel(event.title);
  const unmatched: number[] = [];
  for (let i = session.blocks.length - 1; i >= 0; i--) {
    const block = session.blocks[i];
    if (block.role !== "tool" || block.approval) continue;
    unmatched.push(i);
    const label = normalizeLabel(block.text || block.tool?.title || "");
    if (needle && label === needle) return i;
  }
  return unmatched.length === 1 ? unmatched[0] : -1;
}

function normalizeLabel(value: string): string {
  return value
    .replace(/[→`]/g, "")
    .replace(/\s*\([^)]*\)\s*$/, "")
    .replace(/\s*·.*$/, "")
    .trim()
    .toLowerCase();
}

function upsertTool(
  session: Session,
  patch: {
    callId: string;
    title?: string;
    kind?: string;
    status?: string;
    detail?: string;
    preview?: ToolPreview;
    streaming: boolean;
  },
): Session {
  const index = findToolIndex(session, patch);
  if (index < 0) {
    const detail = capToolDetail(patch.detail);
    const preview = fillPreview(patch.preview, detail, patch.kind, patch.title);
    const label = finalToolLabel(
      session,
      patch.kind,
      displayLabel(patch),
      preview,
    );
    return appendBlock(session, {
      id: crypto.randomUUID(),
      role: "tool",
      text: label,
      streaming: patch.streaming,
      tool: {
        callId: patch.callId,
        title: label,
        kind: patch.kind,
        status: patch.status,
        ...(detail ? { detail } : {}),
        ...(preview ? { preview } : {}),
      },
    });
  }
  const prev = session.blocks[index];
  const detail = capToolDetail(patch.detail) ?? prev.tool?.detail;
  const preview = fillPreview(
    mergeToolPreview(patch.preview, prev.tool?.preview),
    detail,
    patch.kind ?? prev.tool?.kind,
    patch.title,
  );
  const label = finalToolLabel(
    session,
    patch.kind ?? prev.tool?.kind,
    displayLabel(patch, prev),
    preview,
  );
  const kind = patch.kind ?? prev.tool?.kind;
  const status = patch.status ?? prev.tool?.status;
  if (
    prev.text === label &&
    prev.streaming === patch.streaming &&
    prev.tool?.title === label &&
    prev.tool?.kind === kind &&
    prev.tool?.status === status &&
    prev.tool?.detail === detail &&
    samePreview(prev.tool?.preview, preview)
  ) {
    return session;
  }
  const blocks = session.blocks.slice();
  blocks[index] = {
    ...prev,
    text: label,
    streaming: patch.streaming,
    tool: {
      callId: patch.callId,
      title: label,
      kind,
      status,
      ...(detail ? { detail } : {}),
      ...(preview ? { preview } : {}),
    },
  };
  return { ...session, blocks };
}

const MAX_TOOL_DETAIL_CHARS = 8_000;

function capToolDetail(value: string | undefined): string | undefined {
  const text = value?.trim();
  if (!text) return undefined;
  if (text.length <= MAX_TOOL_DETAIL_CHARS) return text;
  return `${text.slice(0, MAX_TOOL_DETAIL_CHARS)}\n…`;
}

function samePreview(a?: ToolPreview, b?: ToolPreview): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.kind === b.kind &&
    a.path === b.path &&
    a.query === b.query &&
    a.fileName === b.fileName &&
    a.additions === b.additions &&
    a.deletions === b.deletions &&
    a.startLine === b.startLine &&
    a.output === b.output &&
    a.lines === b.lines
  );
}

function fillPreview(
  preview: ToolPreview | undefined,
  _detail: string | undefined,
  kind?: string,
  title?: string,
): ToolPreview | undefined {
  if (
    preview?.lines?.some((line) => line.kind === "add" || line.kind === "del")
  ) {
    return preview;
  }
  if (preview) return { ...preview, lines: undefined };
  if (isFileTool(kind, title, preview)) {
    return stubFilePreview(kind, title);
  }
  return undefined;
}

function findToolIndex(
  session: Session,
  patch: { callId: string; title?: string },
): number {
  if (patch.callId) {
    const byId = session.blocks.findIndex(
      (block) => block.tool?.callId === patch.callId,
    );
    if (byId >= 0) return byId;
  }
  const needle = normalizeLabel(patch.title || "");
  if (!needle) return -1;
  return session.blocks.findIndex((block) => {
    if (block.role !== "tool" || !block.approval || block.tool?.callId) {
      return false;
    }
    return normalizeLabel(block.text || block.tool?.title || "") === needle;
  });
}

function sealLastStream(blocks: Block[]): Block[] {
  const last = blocks[blocks.length - 1];
  if (
    !last?.streaming ||
    (last.role !== "assistant" && last.role !== "reasoning")
  ) {
    return blocks.slice();
  }
  const next = blocks.slice();
  next[next.length - 1] = { ...last, streaming: false };
  return next;
}

function displayLabel(
  patch: { title?: string; kind?: string },
  prev?: Block,
): string {
  return (
    preferLabel(patch.title, prev?.tool?.title, prev?.text) ||
    kindTitle(patch.kind ?? prev?.tool?.kind)
  );
}

function finalToolLabel(
  session: Session,
  kind: string | undefined,
  title: string | undefined,
  preview?: ToolPreview,
): string {
  const path = preview?.path
    ? displayPath(preview.path, session.cwd)
    : preview?.fileName;
  return (
    composeToolTitle({
      kind,
      title,
      path,
      query: preview?.query,
      previewKind: preview?.kind,
    }) ||
    title?.trim() ||
    kindTitle(kind)
  );
}

function preferLabel(...parts: (string | undefined)[]): string {
  const filled = parts
    .filter((part): part is string => !!part?.trim())
    .map((part) => part.trim())
    .filter((part) => !isCallId(part));
  const strong = filled.filter(
    (part) => !isWeakToolTitle(part) && compactLabel(part) === part,
  );
  strong.sort((a, b) => b.length - a.length);
  if (strong[0]) return strong[0];
  const compact = filled.filter((part) => compactLabel(part) === part);
  compact.sort((a, b) => b.length - a.length);
  return compact[0] ?? filled[0] ?? "";
}

function compactLabel(value: string | undefined): string | undefined {
  if (!value?.trim()) return undefined;
  const trimmed = value.trim();
  if (trimmed.includes("\n") || trimmed.length > 240) return undefined;
  return trimmed;
}

function kindTitle(kind?: string): string {
  const key = kind?.trim().toLowerCase() ?? "";
  switch (key) {
    case "read":
      return "Read";
    case "edit":
      return "Edit";
    case "delete":
      return "Delete";
    case "move":
      return "Move";
    case "search":
      return "Find";
    case "execute":
      return "Shell";
    case "think":
      return "Think";
    case "fetch":
      return "Fetch";
    case "other":
    case "":
      return "Working";
    default:
      return key.replace(/^_/, "").replace(/[_-]+/g, " ");
  }
}

function isCallId(value: string): boolean {
  const text = value.trim();
  return (
    /^(call[-_]?|tool[-_])[a-z0-9_-]+$/i.test(text) ||
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(text)
  );
}

function finishRole(session: Session, role: Block["role"]): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) =>
      block.role === role && block.streaming
        ? { ...block, streaming: false }
        : block,
    ),
  };
}
