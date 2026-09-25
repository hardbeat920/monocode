import { asRecord, stringField } from "./opencodeProtocol";

type Event = Record<string, unknown>;

type ToolCall = {
  sessionID: string;
  messageID?: string;
  tool: string;
  input?: unknown;
  metadata?: unknown;
};

const MAX_TRACKED = 512;

/**
 * OpenCode 2.x streams durable session events (`session.text.delta`,
 * `session.tool.success`, ...) instead of v1 message/part snapshots. This
 * rebuilds the v1 shapes the provider already consumes so both generations
 * share one transcript path.
 */
export class OpenCodeV2EventTranslator {
  private readonly models = new Map<string, Record<string, unknown>>();
  private readonly startedParts = new Set<string>();
  private readonly tools = new Map<string, ToolCall>();

  translate(event: Event): Event | null {
    const type = stringField(event, "type") ?? "";
    const data = asRecord(event.properties) ?? {};
    const sessionID = stringField(data, "sessionID");
    const messageID = stringField(data, "assistantMessageID");

    switch (type) {
      case "session.created":
        if (!sessionID) return null;
        return {
          type,
          properties: {
            info: {
              id: sessionID,
              parentID: stringField(data, "parentID"),
            },
          },
        };
      case "session.step.started": {
        if (!sessionID || !messageID) return null;
        const model = asRecord(data.model);
        const info = {
          id: messageID,
          sessionID,
          role: "assistant",
          agent: stringField(data, "agent"),
          providerID: stringField(model, "providerID"),
          modelID: stringField(model, "id"),
        };
        remember(this.models, messageID, info);
        return messageUpdated(info);
      }
      case "session.step.ended":
      case "session.step.failed": {
        if (!sessionID || !messageID) return null;
        const info = {
          ...(this.models.get(messageID) ?? {
            id: messageID,
            sessionID,
            role: "assistant",
          }),
          ...(data.tokens !== undefined ? { tokens: data.tokens } : {}),
          ...(data.cost !== undefined ? { cost: data.cost } : {}),
        };
        this.models.delete(messageID);
        return messageUpdated(info);
      }
      case "session.text.started":
      case "session.reasoning.started":
      case "session.text.delta":
      case "session.reasoning.delta":
      case "session.text.ended":
      case "session.reasoning.ended":
        return this.textEvent(type, data);
      case "session.tool.input.started":
      case "session.tool.called":
      case "session.tool.progress":
      case "session.tool.success":
      case "session.tool.failed":
        return this.toolEvent(type, data);
      case "session.execution.succeeded":
      case "session.execution.interrupted":
        return sessionID ? { type: "session.idle", properties: { sessionID } } : null;
      case "session.execution.failed":
      case "session.error":
        return sessionID
          ? { type: "session.error", properties: { sessionID, error: data.error } }
          : null;
      case "session.retry.scheduled": {
        if (!sessionID) return null;
        const error = asRecord(data.error);
        const attempt = typeof data.attempt === "number" ? data.attempt : undefined;
        const reason = stringField(error, "message");
        return {
          type: "session.status",
          properties: {
            sessionID,
            status: {
              type: "retry",
              message: `Retrying${attempt ? ` (attempt ${attempt})` : ""}${reason ? `: ${reason}` : ""}`,
            },
          },
        };
      }
      default:
        return event;
    }
  }

  private textEvent(type: string, data: Event): Event | null {
    const sessionID = stringField(data, "sessionID");
    const messageID = stringField(data, "assistantMessageID");
    if (!sessionID || !messageID) return null;
    const partType = type.startsWith("session.reasoning.") ? "reasoning" : "text";
    const ordinal = typeof data.ordinal === "number" ? data.ordinal : 0;
    const partID = `${messageID}:${partType}:${ordinal}`;
    const part = { id: partID, sessionID, messageID, type: partType };

    if (type.endsWith(".delta")) {
      // Deltas can be whitespace-only; stringField trims, so read the raw value.
      const delta = typeof data.delta === "string" ? data.delta : "";
      if (delta.length === 0) return null;
      if (this.startedParts.has(partID)) {
        return {
          type: "message.part.delta",
          properties: { sessionID, messageID, partID, field: "text", delta },
        };
      }
      // A delta can arrive without its `started` (e.g. a mid-turn subscribe).
      rememberSet(this.startedParts, partID);
      return partUpdated({ ...part, text: delta });
    }
    if (type.endsWith(".ended")) {
      this.startedParts.delete(partID);
      const text = typeof data.text === "string" ? data.text : undefined;
      return text === undefined ? null : partUpdated({ ...part, text });
    }
    rememberSet(this.startedParts, partID);
    return partUpdated({ ...part, text: "" });
  }

  private toolEvent(type: string, data: Event): Event | null {
    const sessionID = stringField(data, "sessionID");
    const callID = stringField(data, "id");
    if (!sessionID || !callID) return null;
    const key = `${sessionID}:${callID}`;
    const existing = this.tools.get(key);
    const call: ToolCall = {
      sessionID,
      messageID: stringField(data, "assistantMessageID") ?? existing?.messageID,
      tool: stringField(data, "name") ?? existing?.tool ?? "tool",
      input: data.input ?? existing?.input,
      metadata: data.metadata ?? existing?.metadata,
    };
    let state: Record<string, unknown>;
    if (type === "session.tool.input.started") {
      state = { status: "pending", input: call.input ?? {} };
    } else if (type === "session.tool.called" || type === "session.tool.progress") {
      state = { status: "running", input: call.input ?? {}, metadata: call.metadata };
    } else {
      const output = contentText(data.content);
      state =
        type === "session.tool.success"
          ? { status: "completed", input: call.input ?? {}, output, metadata: call.metadata }
          : {
              status: "error",
              input: call.input ?? {},
              output,
              metadata: call.metadata,
              error: data.error,
            };
    }
    if (type === "session.tool.success" || type === "session.tool.failed") {
      this.tools.delete(key);
    } else {
      remember(this.tools, key, call);
    }
    return partUpdated({
      id: callID,
      sessionID,
      messageID: call.messageID,
      type: "tool",
      callID,
      tool: call.tool,
      state,
    });
  }
}

/** Text of a v2 tool result's content blocks. */
export function openCodeV2ContentText(content: unknown): string | undefined {
  return contentText(content);
}

function contentText(content: unknown): string | undefined {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return undefined;
  const text = content
    .map((item) => {
      const rec = asRecord(item);
      return stringField(rec, "type") === "text" ? stringField(rec, "text") : undefined;
    })
    .filter((item): item is string => item !== undefined)
    .join("");
  return text || undefined;
}

function messageUpdated(info: Record<string, unknown>): Event {
  return {
    type: "message.updated",
    properties: { sessionID: info.sessionID, info },
  };
}

function partUpdated(part: Record<string, unknown>): Event {
  return {
    type: "message.part.updated",
    properties: { sessionID: part.sessionID, part },
  };
}

function remember<V>(map: Map<string, V>, key: string, value: V): void {
  map.set(key, value);
  if (map.size <= MAX_TRACKED) return;
  const oldest = map.keys().next().value;
  if (oldest !== undefined) map.delete(oldest);
}

function rememberSet(set: Set<string>, key: string): void {
  set.add(key);
  if (set.size <= MAX_TRACKED) return;
  const oldest = set.values().next().value;
  if (oldest !== undefined) set.delete(oldest);
}
