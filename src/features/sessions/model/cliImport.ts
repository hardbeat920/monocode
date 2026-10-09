import { invoke } from "@tauri-apps/api/core";
import {
  applyHarnessEvents,
  appendUser,
  stopStreaming,
} from "../../../integrations/harness/core/apply";
import type { HarnessEvent } from "../../../integrations/harness/core/types";
import {
  previewFromTool,
  toolKindFromName,
  toolTitle,
} from "../../../integrations/harness/providers/claude/claudeProtocol";
import { codexCommandPresentation } from "../../../integrations/harness/providers/codex/codexProtocol";
import {
  readCliSession,
  type CliEntry,
  type CliSession,
  type CliToolEntry,
} from "../../../platform/tauri/cliSessions";
import {
  sanitizeSessionForPersist,
  type SessionSummary,
} from "../data/sessionStore";
import { newSession, type Session } from "./session";

/**
 * Rebuild a CLI transcript as a MonoCode session. The session keeps the
 * CLI's own id as `providerSessionId`, so the next message resumes the same
 * conversation the terminal used, and the CLI keeps seeing it too.
 */
export function sessionFromCliEntries(
  source: CliSession,
  entries: readonly CliEntry[],
  projectCwd: string,
): Session {
  let session: Session = {
    ...newSession(source.harness, projectCwd, source.model),
    title: source.title,
    providerSessionId: source.providerSessionId,
  };
  let lastAt = source.createdAt;
  for (const entry of entries) {
    const at = entry.at ?? lastAt;
    lastAt = Math.max(lastAt, at);
    if (entry.kind === "user") {
      if (session.busy) session = stopStreaming(session, at);
      session = appendUser(session, entry.text);
      session = stampLastBlock(session, at);
      continue;
    }
    session = applyHarnessEvents(session, eventsForEntry(source, entry));
  }
  session = stopStreaming(session, Math.max(lastAt, source.updatedAt));
  return { ...session, busy: false };
}

function eventsForEntry(
  source: CliSession,
  entry: CliEntry,
): HarnessEvent[] {
  switch (entry.kind) {
    case "assistant":
      return [
        { type: "message.delta", text: entry.text },
        { type: "message.completed" },
      ];
    case "reasoning":
      return [
        { type: "reasoning.delta", text: entry.text },
        { type: "reasoning.completed" },
      ];
    case "tool":
      return toolEvents(source, entry);
    default:
      return [];
  }
}

function toolEvents(source: CliSession, tool: CliToolEntry): HarnessEvent[] {
  const callId = tool.id || crypto.randomUUID();
  const { title, kind, preview } = toolPresentation(source, tool);
  return [
    { type: "tool.started", callId, title, kind, preview },
    {
      type: "tool.updated",
      callId,
      status: tool.failed ? "failed" : "completed",
      ...(tool.output ? { detail: tool.output } : {}),
    },
  ];
}

/** Title, kind and preview the live adapter would have shown for this call. */
function toolPresentation(
  source: CliSession,
  tool: CliToolEntry,
): Pick<
  Extract<HarnessEvent, { type: "tool.started" }>,
  "title" | "kind" | "preview"
> {
  if (source.harness === "claude") {
    const input = isRecord(tool.input) ? tool.input : {};
    return {
      title: toolTitle(tool.name, input),
      kind: toolKindFromName(tool.name),
      preview: previewFromTool(tool.name, input, tool.output),
    };
  }
  if (tool.command) {
    const { title, preview } = codexCommandPresentation(
      { cwd: source.cwd },
      tool.command,
    );
    return { title, kind: "execute", preview };
  }
  return {
    title: tool.title || tool.name,
    kind: tool.toolKind,
  };
}

function stampLastBlock(session: Session, at: number): Session {
  const blocks = session.blocks.slice();
  const last = blocks[blocks.length - 1];
  if (last) blocks[blocks.length - 1] = { ...last, startedAt: at };
  return { ...session, blocks };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

/**
 * Read, rebuild and save one CLI session. Returns `null` when MonoCode
 * already has a row for that provider session or the transcript has no
 * message the user typed.
 */
export async function importCliSession(
  source: CliSession,
  projectCwd: string,
): Promise<SessionSummary | null> {
  const entries = await readCliSession(source);
  const session = sessionFromCliEntries(source, entries, projectCwd);
  if (!session.blocks.some((block) => block.role === "user")) return null;
  return invoke<SessionSummary | null>("session_import", {
    session: sanitizeSessionForPersist(session),
    createdAt: source.createdAt,
    updatedAt: source.updatedAt,
  });
}

type ImportListener = (cwd: string) => void;
const importListeners = new Set<ImportListener>();

/** Called after sessions were imported into `cwd`, so history can reload. */
export function subscribeCliSessionsImported(
  listener: ImportListener,
): () => void {
  importListeners.add(listener);
  return () => importListeners.delete(listener);
}

export function notifyCliSessionsImported(cwd: string): void {
  for (const listener of importListeners) listener(cwd);
}
