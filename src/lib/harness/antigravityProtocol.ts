import type { AgentModel, ModelSetting } from "../models";
import type { RuntimeMode } from "../session";
import { extractToolPreview } from "./preview";
import type { HarnessEvent } from "./types";

export type AntigravityInitEvent = {
  type: "init";
  conversationId: string;
  model?: string;
};

export type AntigravityStepUpdateEvent = {
  type: "step_update";
  conversationId?: string;
  stepIndex: number;
  state: "ACTIVE" | "DONE" | "ERROR";
  stepType: string;
  textDelta?: string;
  toolName?: string;
  toolInfo?: {
    name?: string;
    parameters?: Record<string, unknown>;
    output?: unknown;
    error?: unknown;
  };
  durationSeconds?: number;
  usage?: {
    inputTokens?: number;
    outputTokens?: number;
    thinkingTokens?: number;
    cacheReadTokens?: number;
    totalTokens?: number;
  };
};

export type AntigravityResultEvent = {
  type: "result";
  conversationId?: string;
  status: "SUCCESS" | "ERROR";
  response?: string;
  error?: string;
  durationSeconds?: number;
  numTurns?: number;
  usage?: {
    inputTokens?: number;
    outputTokens?: number;
    thinkingTokens?: number;
    cacheReadTokens?: number;
    totalTokens?: number;
  };
};

export type AntigravityErrorEvent = {
  type: "error";
  error: string;
};

export type AntigravityParsedLine =
  | AntigravityInitEvent
  | AntigravityStepUpdateEvent
  | AntigravityResultEvent
  | AntigravityErrorEvent;

export function buildAntigravitySpawnArgs(input: {
  model?: string;
  conversationId?: string;
  runtimeMode?: RuntimeMode;
  effort?: string;
}): string[] {
  const args = [
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
  ];
  if (input.model) {
    args.push("--model", input.model);
  }
  if (input.conversationId) {
    args.push("--conversation", input.conversationId);
  }
  if (input.effort) {
    args.push("--effort", input.effort);
  }
  if (input.runtimeMode === "full-access" || input.runtimeMode === "auto") {
    args.push("--dangerously-skip-permissions");
  } else if (input.runtimeMode === "auto-accept-edits" || input.runtimeMode === "supervised") {
    args.push("--mode", "accept-edits");
  }
  return args;
}

export function buildAntigravityUserMessage(input: {
  text: string;
}): string {
  return JSON.stringify({
    event: "user",
    message: {
      role: "user",
      content: input.text,
    },
  });
}

export function parseAntigravityLine(line: string): AntigravityParsedLine | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const data = JSON.parse(trimmed) as Record<string, unknown>;
    const event = data.event;
    if (event === "init") {
      const initObj = (data.init as Record<string, unknown>) ?? {};
      return {
        type: "init",
        conversationId: String(data.conversation_id ?? ""),
        model: typeof initObj.model === "string" ? initObj.model : undefined,
      };
    }
    if (event === "step_update") {
      const step = (data.step_update as Record<string, unknown>) ?? {};
      const usageObj = step.usage as Record<string, unknown> | undefined;
      const toolInfo = step.tool_info as Record<string, unknown> | undefined;
      const rawState = String(step.state ?? "").toUpperCase();
      const state: "ACTIVE" | "DONE" | "ERROR" =
        rawState === "ACTIVE" ? "ACTIVE" : rawState === "ERROR" ? "ERROR" : "DONE";

      const inputTokens = typeof usageObj?.input_tokens === "number" ? usageObj.input_tokens : undefined;
      const outputTokens = typeof usageObj?.output_tokens === "number" ? usageObj.output_tokens : undefined;
      const totalTokens =
        typeof usageObj?.total_tokens === "number"
          ? usageObj.total_tokens
          : inputTokens != null || outputTokens != null
            ? (inputTokens ?? 0) + (outputTokens ?? 0)
            : undefined;

      return {
        type: "step_update",
        conversationId: typeof step.conversation_id === "string" ? step.conversation_id : undefined,
        stepIndex: typeof step.step_index === "number" ? step.step_index : 0,
        state,
        stepType: String(step.step_type ?? ""),
        textDelta: typeof step.text_delta === "string" ? step.text_delta : undefined,
        toolName: typeof step.tool_name === "string" ? step.tool_name : undefined,
        toolInfo: toolInfo
          ? {
              name: typeof toolInfo.name === "string" ? toolInfo.name : undefined,
              parameters: (toolInfo.parameters as Record<string, unknown>) ?? undefined,
              output: toolInfo.output,
              error: toolInfo.error,
            }
          : undefined,
        durationSeconds: typeof step.duration_seconds === "number" ? step.duration_seconds : undefined,
        usage: usageObj
          ? {
              inputTokens,
              outputTokens,
              thinkingTokens: typeof usageObj.thinking_tokens === "number" ? usageObj.thinking_tokens : undefined,
              cacheReadTokens: typeof usageObj.cache_read_tokens === "number" ? usageObj.cache_read_tokens : undefined,
              totalTokens,
            }
          : undefined,
      };
    }
    if (event === "result") {
      const result = (data.result as Record<string, unknown>) ?? {};
      const usageObj = result.usage as Record<string, unknown> | undefined;
      const rawStatus = String(result.status ?? "").toUpperCase();
      const status: "SUCCESS" | "ERROR" = rawStatus === "ERROR" ? "ERROR" : "SUCCESS";

      const inputTokens = typeof usageObj?.input_tokens === "number" ? usageObj.input_tokens : undefined;
      const outputTokens = typeof usageObj?.output_tokens === "number" ? usageObj.output_tokens : undefined;
      const totalTokens =
        typeof usageObj?.total_tokens === "number"
          ? usageObj.total_tokens
          : inputTokens != null || outputTokens != null
            ? (inputTokens ?? 0) + (outputTokens ?? 0)
            : undefined;

      return {
        type: "result",
        conversationId: typeof result.conversation_id === "string" ? result.conversation_id : undefined,
        status,
        response: typeof result.response === "string" ? result.response : undefined,
        error: typeof result.error === "string" ? result.error : undefined,
        durationSeconds: typeof result.duration_seconds === "number" ? result.duration_seconds : undefined,
        numTurns: typeof result.num_turns === "number" ? result.num_turns : undefined,
        usage: usageObj
          ? {
              inputTokens,
              outputTokens,
              thinkingTokens: typeof usageObj.thinking_tokens === "number" ? usageObj.thinking_tokens : undefined,
              cacheReadTokens: typeof usageObj.cache_read_tokens === "number" ? usageObj.cache_read_tokens : undefined,
              totalTokens,
            }
          : undefined,
      };
    }
    if (event === "error") {
      return {
        type: "error",
        error: String(data.error ?? data.message ?? "Antigravity error"),
      };
    }
    return null;
  } catch {
    return null;
  }
}

export function mapAntigravityEvents(
  parsed: AntigravityParsedLine,
  callIdPrefix = "tool",
): HarnessEvent[] {
  switch (parsed.type) {
    case "init": {
      if (parsed.conversationId) {
        return [
          {
            type: "session.providerBound",
            providerSessionId: parsed.conversationId,
          },
        ];
      }
      return [];
    }
    case "step_update": {
      const events: HarnessEvent[] = [];
      if (parsed.stepType === "agent_response") {
        if (parsed.textDelta) {
          events.push({ type: "message.delta", text: parsed.textDelta });
        }
        if (parsed.usage?.totalTokens != null) {
          events.push({ type: "context", used: parsed.usage.totalTokens });
        }
      } else if (parsed.stepType === "reasoning" || parsed.stepType === "thought") {
        if (parsed.textDelta) {
          events.push({ type: "reasoning.delta", text: parsed.textDelta });
        }
      } else if (parsed.stepType === "tool") {
        const callId = `${callIdPrefix}-${parsed.stepIndex}`;
        const title = parsed.toolName ?? parsed.toolInfo?.name ?? "tool";
        const toolObj = {
          name: title,
          ...(parsed.toolInfo?.parameters ?? {}),
        };
        if (parsed.state === "ACTIVE") {
          events.push({
            type: "tool.started",
            callId,
            title,
            preview: extractToolPreview({}, toolObj),
          });
        } else {
          events.push({
            type: "tool.updated",
            callId,
            status: parsed.state === "ERROR" ? "failed" : "completed",
            preview: extractToolPreview(
              {},
              {
                ...toolObj,
                output: parsed.toolInfo?.output,
                error: parsed.toolInfo?.error,
              },
            ),
          });
        }
      }
      return events;
    }
    case "result": {
      const events: HarnessEvent[] = [];
      if (parsed.status === "ERROR") {
        events.push({
          type: "session.error",
          message: parsed.error || "Antigravity execution failed",
        });
      }
      events.push({ type: "message.completed" });
      if (parsed.usage?.totalTokens != null) {
        events.push({ type: "context", used: parsed.usage.totalTokens });
      }
      return events;
    }
    case "error": {
      return [
        {
          type: "session.error",
          message: parsed.error,
        },
        { type: "message.completed" },
      ];
    }
  }
}

const ANSI_REGEX = /\x1B\[[0-9;]*[a-zA-Z]/g;

export function parseAntigravityModels(output: string): AgentModel[] {
  const rawModels: { nativeId: string; name: string }[] = [];
  const lines = output.split("\n");
  for (const line of lines) {
    const clean = line.replace(ANSI_REGEX, "").trim();
    if (!clean || clean.startsWith("Fetching")) continue;
    if (/^(model(\s+id)?|name|---+)/i.test(clean)) continue;

    const tabIndex = clean.indexOf("\t");
    let nativeId = "";
    let name = "";
    if (tabIndex !== -1) {
      nativeId = clean.slice(0, tabIndex).trim();
      name = clean.slice(tabIndex + 1).trim();
    } else {
      const parts = clean.split(/\s{2,}/);
      if (parts.length >= 2) {
        nativeId = parts[0].trim();
        name = parts.slice(1).join(" ").trim();
      } else {
        nativeId = clean;
        name = clean;
      }
    }
    if (nativeId) {
      rawModels.push({ nativeId, name: name || nativeId });
    }
  }

  const grouped = new Map<
    string,
    {
      baseNativeId: string;
      baseName: string;
      efforts: { value: string; label: string }[];
    }
  >();

  for (const raw of rawModels) {
    const match = raw.nativeId.match(/^(.+)-(high|medium|low)$/i);
    if (match) {
      const baseId = match[1];
      const effortLevel = match[2].toLowerCase();
      const effortLabel =
        effortLevel === "high"
          ? "High"
          : effortLevel === "medium"
            ? "Medium"
            : "Low";
      const baseName = raw.name.replace(/\s*\((High|Medium|Low)\)$/i, "").trim();

      const existing = grouped.get(baseId);
      if (existing) {
        if (!existing.efforts.some((e) => e.value === effortLevel)) {
          existing.efforts.push({ value: effortLevel, label: effortLabel });
        }
      } else {
        grouped.set(baseId, {
          baseNativeId: baseId,
          baseName: baseName || baseId,
          efforts: [{ value: effortLevel, label: effortLabel }],
        });
      }
    } else {
      grouped.set(raw.nativeId, {
        baseNativeId: raw.nativeId,
        baseName: raw.name,
        efforts: [],
      });
    }
  }

  const effortOrder = ["high", "medium", "low"];
  const models: AgentModel[] = [];

  for (const [, info] of grouped) {
    info.efforts.sort(
      (a, b) => effortOrder.indexOf(a.value) - effortOrder.indexOf(b.value),
    );

    const settings: ModelSetting[] | undefined =
      info.efforts.length > 0
        ? [
            {
              id: "effort",
              label: "Reasoning",
              kind: "select",
              value:
                info.efforts.find((e) => e.value === "high")?.value ??
                info.efforts[0].value,
              options: info.efforts,
            },
          ]
        : undefined;

    models.push({
      id: `antigravity:${info.baseNativeId}`,
      harness: "antigravity",
      name: info.baseName,
      nativeId: info.baseNativeId,
      settings,
    });
  }

  return models;
}
