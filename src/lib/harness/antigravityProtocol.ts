import { nativeModelId } from "../models";
import type { Attachment, RuntimeMode, ToolPreview } from "../session";
import { extractToolPreview, titleFromToolInput } from "./preview";
import { snapshotRemainder } from "./streamText";
import type { HarnessEvent } from "./types";

/** The documented streaming protocol is available in agy 1.1.15 and later. */
export const ANTIGRAVITY_MINIMUM_VERSION = "1.1.24";

// agy's five-minute print-mode default applies to each stream-json turn too.
// Match the other long-running harnesses so active tool work is not cut short.
const ANTIGRAVITY_TURN_TIMEOUT = "30m";

type RecordValue = Record<string, unknown>;

export function asRecord(value: unknown): RecordValue | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as RecordValue)
    : null;
}

export function stringField(
  value: RecordValue | null | undefined,
  key: string,
): string | undefined {
  const found = value?.[key];
  return typeof found === "string" && found.trim() ? found : undefined;
}

export function parseAntigravityLine(line: string): RecordValue | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    return asRecord(JSON.parse(trimmed));
  } catch {
    return null;
  }
}

export function antigravityMode(mode: RuntimeMode): string | undefined {
  // Antigravity has no equivalent to MonoCode's reviewer-driven Auto mode.
  // `accept-edits` is its documented non-interactive edit approval mode.
  return mode === "auto-accept-edits" ? "accept-edits" : undefined;
}

import { questionsFromUnknown } from "../userQuestion";
import type { AgentModel } from "../models";

export function resolveAntigravityModelWithEffort(
  rawModel: string,
  effort?: string,
): { model: string; effort?: string } {
  const model = nativeModelId(rawModel).trim();
  if (!model || model === "default") {
    return {
      model: "",
      effort: effort && ["low", "medium", "high"].includes(effort) ? effort : undefined,
    };
  }

  const selectedEffort = effort?.trim().toLowerCase();
  const eff =
    selectedEffort && ["low", "medium", "high"].includes(selectedEffort)
      ? selectedEffort
      : undefined;

  // Check if model already has a reasoning suffix (e.g. gemini-3.8-flash-high)
  const match = model.match(/^(gemini-[\d.]+(?:-(?:flash|pro)))-(high|medium|low)$/);
  if (match) {
    const base = match[1];
    const finalEffort = eff ?? match[2];
    return { model: `${base}-${finalEffort}`, effort: finalEffort };
  }

  if (model.startsWith("gemini-")) {
    const finalEff = eff ?? "high";
    if (model === "gemini-3.1-pro") {
      const proEff = finalEff === "medium" ? "high" : finalEff;
      return { model: `gemini-3.1-pro-${proEff}`, effort: proEff };
    }
    return { model: `${model}-${finalEff}`, effort: finalEff };
  }

  return {
    model,
    effort: eff,
  };
}

export function effectiveAntigravitySettings(input: {
  model: string;
  modelSettings?: Record<string, string>;
  runtimeMode: RuntimeMode;
  prompt?: string;
}): {
  model: string;
  effort?: string;
  agent?: string;
  mode?: string;
} {
  const isBoosted = input.prompt?.includes("/boost");
  const requestedEffort =
    input.modelSettings?.effort?.trim() || (isBoosted ? "high" : undefined);
  const resolved = resolveAntigravityModelWithEffort(input.model, requestedEffort);
  const agent = input.modelSettings?.agent?.trim() || undefined;
  const isPlanning =
    input.prompt?.includes("/plan") ||
    input.modelSettings?.mode === "plan" ||
    (input.runtimeMode as string) === "plan";
  const mode = isPlanning ? "plan" : antigravityMode(input.runtimeMode);

  return {
    model: resolved.model,
    effort: resolved.effort,
    agent,
    mode,
  };
}

export function buildAntigravitySpawnArgs(input: {
  model: string;
  modelSettings?: Record<string, string>;
  runtimeMode: RuntimeMode;
  resume?: string;
  cwd?: string;
  addDirs?: string[];
  prompt?: string;
}): string[] {
  const args = [
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--print-timeout",
    ANTIGRAVITY_TURN_TIMEOUT,
    "--disable-slash-commands",
  ];
  const effective = effectiveAntigravitySettings({
    model: input.model,
    modelSettings: input.modelSettings,
    runtimeMode: input.runtimeMode,
    prompt: input.prompt,
  });

  if (effective.model) args.push("--model", effective.model);
  if (effective.effort) args.push("--effort", effective.effort);
  if (effective.agent) args.push("--agent", effective.agent);
  if (effective.mode) args.push("--mode", effective.mode);

  if (input.runtimeMode === "full-access") {
    args.push("--dangerously-skip-permissions");
  } else {
    // Keep the CLI's native OS containment enabled for normal sessions. The
    // working directory identifies the workspace; sandboxing makes that
    // boundary meaningful for agent-launched processes as well.
    args.push("--sandbox");
  }
  if (input.resume?.trim()) args.push("--conversation", input.resume.trim());
  const allDirs = [
    ...(input.cwd?.trim() ? [input.cwd.trim()] : []),
    ...(input.addDirs ?? []),
  ];
  for (const dir of uniqueDirs(allDirs))
    args.push("--add-dir", dir);
  return args;
}

function uniqueDirs(dirs: string[]): string[] {
  return [
    ...new Set(
      dirs
        .map((dir) => dir.trim().replace(/[/\\]+$/, ""))
        .filter(Boolean),
    ),
  ];
}

function isAbsolutePath(path: string): boolean {
  return (
    path.startsWith("/") ||
    /^[A-Za-z]:[/\\]/.test(path) ||
    path.startsWith("\\\\")
  );
}

function joinPath(base: string, rel: string): string {
  const cleanBase = base.replace(/[/\\]+$/, "");
  const cleanRel = rel.replace(/^[/\\]+/, "");
  return `${cleanBase}/${cleanRel}`;
}

/** `stream-json` accepts only text blocks. Files are made visible through @refs. */
export function buildAntigravityUserMessage(input: {
  text: string;
  cwd?: string;
  attachments?: Attachment[];
}): RecordValue | null {
  const references = (input.attachments ?? []).flatMap((attachment) => {
    let path = attachment.path?.trim();
    if (path && input.cwd && !isAbsolutePath(path)) {
      path = joinPath(input.cwd, path);
    }
    return path ? [`@[${path}]`] : [];
  });
  const text = [
    references.length
      ? "Attachments to inspect:\n" + references.join("\n")
      : "",
    input.text.trim(),
  ]
    .filter(Boolean)
    .join("\n\n");
  if (!text) return null;
  return { event: "user", message: { content: text } };
}

export function attachmentDirs(
  attachments: Attachment[] | undefined,
): string[] {
  return [
    ...new Set(
      (attachments ?? [])
        .flatMap((attachment) => {
          const path = attachment.path?.trim();
          return path ? [parentDir(path)] : [];
        })
        .filter(Boolean),
    ),
  ];
}

function parentDir(path: string): string {
  const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return slash > 0 ? path.slice(0, slash) : "";
}

export type AntigravityMappedLine = {
  initialized?: string;
  providerSessionId?: string;
  events: HarnessEvent[];
  turnCompleted?: { ok: boolean; error?: string; response?: string };
};

export function antigravityContextWindow(modelId?: string): number {
  const lower = modelId?.toLowerCase() ?? "";
  if (
    lower.includes("claude") ||
    lower.includes("sonnet") ||
    lower.includes("opus")
  ) {
    return 200_000;
  }
  if (lower.includes("gpt") || lower.includes("oss")) {
    return 128_000;
  }
  return 1_000_000;
}

/** Translate a documented NDJSON event into MonoCode's provider-neutral events. */
export function mapAntigravityLine(
  line: RecordValue,
  emittedAssistant = "",
  contextWindow?: number,
  emittedThinking = "",
): AntigravityMappedLine {
  const event = stringField(line, "event");
  if (event === "init") {
    const conversationId = stringField(line, "conversation_id");
    return {
      initialized: conversationId ?? "",
      providerSessionId: conversationId,
      events: [],
    };
  }
  if (event === "step_update") {
    return mapStepUpdate(
      asRecord(line.step_update),
      emittedAssistant,
      emittedThinking,
    );
  }
  if (event === "result") {
    const result = asRecord(line.result);
    const status = stringField(result, "status")?.toUpperCase();
    const response = stringField(result, "response") ?? "";
    const error = stringField(result, "error");
    const ok = status === "SUCCESS";
    const events: HarnessEvent[] = [];
    // Only emit text delta for successful turns; error details are delivered via turnCompleted/session.error
    // to avoid duplicating the error message in the chat stream.
    if (ok) {
      const remainder = snapshotRemainder(emittedAssistant, response);
      if (remainder) events.push({ type: "message.delta", text: remainder });
    }
    events.push({ type: "message.completed" }, { type: "reasoning.completed" });
    const usage = asRecord(result?.usage);
    const used =
      typeof usage?.total_tokens === "number" && usage.total_tokens > 0
        ? usage.total_tokens
        : typeof usage?.input_tokens === "number" &&
            typeof usage?.output_tokens === "number"
          ? usage.input_tokens + usage.output_tokens
          : undefined;
    if (used && used > 0) {
      const window =
        contextWindow && contextWindow > 0
          ? contextWindow
          : antigravityContextWindow();
      events.push({ type: "context", used, window });
    }
    const finalError = !ok
      ? error || (response.trim() ? response.trim() : undefined)
      : undefined;
    return {
      providerSessionId: stringField(result, "conversation_id"),
      events,
      turnCompleted: {
        ok,
        ...(finalError ? { error: finalError } : {}),
        response,
      },
    };
  }
  return { events: [] };
}

function mapStepUpdate(
  step: RecordValue | null,
  emittedAssistant = "",
  emittedThinking = "",
): AntigravityMappedLine {
  if (!step) return { events: [] };
  const type = stringField(step, "step_type")?.toLowerCase();
  const events: HarnessEvent[] = [];

  // 1. Thinking / reasoning from agent_response, thought, thinking, or any step carrying thoughts
  const rawThinkingDelta =
    stringField(step, "thinking_delta") ??
    stringField(step, "thought_delta") ??
    stringField(step, "delta_thinking") ??
    stringField(step, "delta_raw_thinking") ??
    stringField(step, "reasoning_delta");
  const rawThinkingSnapshot =
    stringField(step, "thinking") ??
    stringField(step, "thought") ??
    stringField(step, "raw_thinking") ??
    stringField(step, "raw_thought") ??
    stringField(step, "model_thinking") ??
    stringField(step, "reasoning");
  const thinkingText =
    rawThinkingDelta ??
    (rawThinkingSnapshot
      ? snapshotRemainder(emittedThinking, rawThinkingSnapshot)
      : undefined);
  if (thinkingText) {
    events.push({ type: "reasoning.delta", text: thinkingText });
  }

  // 2. Prose / message text from agent_response, or any step carrying text
  const rawTextDelta =
    stringField(step, "text_delta") ??
    stringField(step, "delta_text") ??
    stringField(step, "delta");
  const rawTextSnapshot =
    stringField(step, "content") ??
    stringField(step, "text") ??
    stringField(step, "message_text") ??
    stringField(step, "message_content") ??
    stringField(step, "display_text");
  const messageText =
    rawTextDelta ??
    (rawTextSnapshot
      ? snapshotRemainder(emittedAssistant, rawTextSnapshot)
      : undefined);
  if (messageText) {
    events.push({ type: "message.delta", text: messageText });
  }

  // If this was not a tool step, return the thinking/message events
  if (type !== "tool") {
    return { events };
  }

  // 3. Tool step
  const name =
    stringField(step, "tool_name") ??
    stringField(asRecord(step.tool_info), "name") ??
    "Tool";
  const index = typeof step.step_index === "number" ? step.step_index : 0;
  const conversationId = stringField(step, "conversation_id") ?? "session";
  const callId = `${conversationId}:${index}`;
  const toolInfo = asRecord(step.tool_info) ?? {};
  const parameters = asRecord(toolInfo.parameters) ?? {};

  const toolAction =
    stringField(parameters, "toolAction") ??
    stringField(parameters, "tool_action") ??
    stringField(toolInfo, "tool_action") ??
    stringField(toolInfo, "toolAction") ??
    stringField(parameters, "toolSummary") ??
    stringField(parameters, "tool_summary") ??
    stringField(toolInfo, "tool_summary") ??
    stringField(toolInfo, "toolSummary") ??
    stringField(parameters, "Description") ??
    stringField(parameters, "description");

  const title =
    toolAction || titleFromToolInput(name, antigravityToolKind(name), parameters);
  const preview = antigravityToolPreview(name, parameters, toolInfo);
  const state = stringField(step, "state")?.toUpperCase();
  const toolError = toolErrorMessage(toolInfo);
  const detail = toolError || toolAction || undefined;

  if (name.toLowerCase() === "ask_question") {
    const questions = questionsFromUnknown(parameters);
    if (questions.length > 0) {
      if (state === "ACTIVE") {
        events.push({
          type: "question.asked",
          requestId: index,
          title: title || "Clarifying question",
          questions,
          callId,
        });
      } else {
        events.push({
          type: "question.resolved",
          requestId: index,
          decision: "answered",
        });
      }
    }
  }

  const targetFile =
    stringField(parameters, "TargetFile") ??
    stringField(parameters, "targetFile") ??
    "";
  const codeContent =
    stringField(parameters, "CodeContent") ??
    stringField(parameters, "codeContent");
  const metadata = asRecord(parameters.ArtifactMetadata);
  if (
    (targetFile.toLowerCase().includes("plan") || metadata?.RequestFeedback === true) &&
    codeContent
  ) {
    events.push({ type: "plan", text: codeContent });
  }

  const stepPlan = stringField(step, "plan");
  if (stepPlan) {
    events.push({ type: "plan", text: stepPlan });
  }

  if (state === "ACTIVE") {
    events.push({
      type: "tool.started",
      callId,
      title,
      kind: antigravityToolKind(name),
      status: "in_progress",
      preview,
    });
    return { events };
  }

  events.push({
    type: "tool.updated",
    callId,
    title,
    kind: antigravityToolKind(name),
    status: toolError ? "failed" : "completed",
    ...(detail ? { detail } : {}),
    preview,
  });
  return { events };
}

export function antigravityToolKind(name: string): string {
  const key = name.toLowerCase();
  if (key === "run_command" || key.includes("command")) return "execute";
  if (key.includes("write") || key.includes("replace") || key.includes("edit"))
    return "edit";
  if (key.includes("grep") || key.includes("search") || key.includes("find"))
    return "search";
  if (
    key.includes("view") ||
    key.includes("read") ||
    key.includes("list_dir") ||
    key.includes("sed_file")
  )
    return "read";
  if (key.includes("url") || key.includes("browser") || key.includes("web"))
    return "fetch";
  if (key.includes("subagent") || key.includes("agent")) return "agent";
  return "other";
}

function antigravityToolPreview(
  name: string,
  parameters: RecordValue,
  info: RecordValue,
): ToolPreview | undefined {
  const content = buildAntigravityToolContent(name, parameters);
  return extractToolPreview(
    {
      name,
      title: name,
      kind: antigravityToolKind(name),
      input: parameters,
      rawInput: parameters,
      ...(content ? { content } : {}),
    },
    {
      name,
      title: name,
      kind: antigravityToolKind(name),
      input: parameters,
      rawInput: parameters,
      output: info.output,
      ...(content ? { content } : {}),
    },
  );
}

function buildAntigravityToolContent(
  name: string,
  parameters: RecordValue,
): RecordValue[] | undefined {
  const key = name.toLowerCase();
  if (key === "replace_file_content") {
    const targetFile =
      stringField(parameters, "TargetFile") ??
      stringField(parameters, "targetFile") ??
      stringField(parameters, "target_file");
    const oldText =
      stringField(parameters, "TargetContent") ??
      stringField(parameters, "targetContent");
    const newText =
      stringField(parameters, "ReplacementContent") ??
      stringField(parameters, "replacementContent");
    if (targetFile && (oldText != null || newText != null)) {
      return [
        {
          type: "diff",
          path: targetFile,
          oldText: oldText ?? "",
          newText: newText ?? "",
        },
      ];
    }
  }
  if (key === "write_to_file") {
    const targetFile =
      stringField(parameters, "TargetFile") ??
      stringField(parameters, "targetFile");
    const newText =
      stringField(parameters, "CodeContent") ??
      stringField(parameters, "codeContent");
    if (targetFile && newText != null) {
      return [
        {
          type: "diff",
          path: targetFile,
          oldText: "",
          newText,
        },
      ];
    }
  }
  return undefined;
}

function toolErrorMessage(info: RecordValue): string | undefined {
  const error = info.error;
  if (typeof error === "string" && error.trim()) return error;
  const rec = asRecord(error);
  return stringField(rec, "message") ?? stringField(rec, "type");
}

export function parseAntigravityModels(
  output: string,
): Array<{ id: string; label: string }> {
  for (const line of output.split(/\r?\n/)) {
    const rec = parseAntigravityLine(line);
    const command = asRecord(rec?.command);
    const data = asRecord(command?.data);
    const models = Array.isArray(data?.models) ? data.models : [];
    if (models.length) {
      return models.flatMap((model) => {
        const value = asRecord(model);
        const id = stringField(value, "id");
        const label = stringField(value, "label");
        return id ? [{ id, label: label ?? id }] : [];
      });
    }
  }
  return [];
}

export function unifyAntigravityCatalogModels(
  rawModels: Array<{ id: string; label: string }>,
): AgentModel[] {
  const groups = new Map<
    string,
    { label: string; efforts: Set<string>; baseId: string }
  >();
  const standalone: AgentModel[] = [];

  for (const { id, label } of rawModels) {
    const match = id.match(
      /^(gemini-[\d.]+(?:-(?:flash|pro)))-(high|medium|low)$/,
    );
    if (match) {
      const baseId = match[1];
      const effort = match[2];
      const cleanLabel = label
        .replace(/\s*\((?:High|Medium|Low)\)\s*$/i, "")
        .trim();
      const existing = groups.get(baseId);
      if (existing) {
        existing.efforts.add(effort);
      } else {
        groups.set(baseId, {
          label: cleanLabel || baseId,
          efforts: new Set([effort]),
          baseId,
        });
      }
    } else {
      standalone.push({
        id: `antigravity:${id}`,
        harness: "antigravity",
        name: label,
        nativeId: id,
        contextWindow: antigravityContextWindow(id),
      });
    }
  }

  const unified: AgentModel[] = [];
  for (const [baseId, { label, efforts }] of groups.entries()) {
    const effortOrder = ["high", "medium", "low"].filter((eff) =>
      efforts.has(eff),
    );
    const effortOptions = (
      effortOrder.length ? effortOrder : ["high", "medium", "low"]
    ).map((eff) => ({
      value: eff,
      label: eff.charAt(0).toUpperCase() + eff.slice(1),
    }));

    unified.push({
      id: `antigravity:${baseId}`,
      harness: "antigravity",
      name: label,
      nativeId: baseId,
      contextWindow: antigravityContextWindow(baseId),
      settings: [
        {
          id: "effort",
          label: "Reasoning",
          kind: "select",
          value: "high",
          options: effortOptions,
        },
      ],
    });
  }

  return [...unified, ...standalone];
}
