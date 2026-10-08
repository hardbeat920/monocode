import type {
  Attachment,
  RuntimeMode,
  ToolPreview,
  TurnMetrics,
} from "../../../../features/sessions/model/session";
import {
  attachmentPath,
  attachmentPathText,
  isVisionImage,
  promptText,
} from "../../../../features/sessions/model/attachments";
import { isTaskListToolName } from "../../../../features/sessions/model/taskList";
import { extractToolPreview } from "../../core/preview";
import type { HarnessEvent } from "../../core/types";

export const MINIMUM_OPENCODE_VERSION = "1.14.19";
export const MINIMUM_OPENCODE_V2_VERSION = "2.0.15";
export const OPENCODE_SERVER_READY_PREFIX = "opencode server listening";
export const KNOWN_HIDDEN_AGENTS = new Set(["compaction", "summary", "title"]);
let lastMessageTimestamp = 0;
let messageCounter = 0;

/** Revert boundaries compare IDs, so use OpenCode's ascending timestamp encoding. */
export function nextOpenCodeMessageId(timestamp = Date.now()): string {
  if (timestamp !== lastMessageTimestamp) {
    lastMessageTimestamp = timestamp;
    messageCounter = 0;
  }
  const time = (
    (BigInt(timestamp) * 0x1000n + BigInt(++messageCounter)) &
    0xffffffffffffn
  )
    .toString(16)
    .padStart(12, "0");
  const chars =
    "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
  const suffix = [...crypto.getRandomValues(new Uint8Array(14))]
    .map((byte) => chars[byte % chars.length])
    .join("");
  return `msg_${time}${suffix}`;
}

export type OpenCodeApiGeneration = "v1" | "v2";

const OPENCODE_DEFAULT_TITLE_PATTERN =
  /^(New session - |Child session - )\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

export type ParsedOpenCodeModelSlug = {
  providerID: string;
  modelID: string;
};

export type OpenCodePermissionRule = {
  permission: string;
  pattern: string;
  action: "allow" | "deny" | "ask";
};

export type OpenCodeV2PermissionRule = {
  action: string;
  resource: string;
  effect: "allow" | "deny" | "ask";
};

export type OpenCodePart = {
  id: string;
  type: string;
  messageID?: string;
  callID?: string;
  tool?: string;
  text?: string;
  time?: { start?: number; end?: number };
  state?: Record<string, unknown>;
};

export function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

export function stringField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): string | undefined {
  if (!rec) return undefined;
  const value = rec[key];
  return typeof value === "string" && value.trim() ? value : undefined;
}

export function parseOpenCodeModelSlug(
  slug: string | null | undefined,
): ParsedOpenCodeModelSlug | null {
  if (typeof slug !== "string") return null;
  const trimmed = slug.trim();
  const separator = trimmed.indexOf("/");
  if (separator <= 0 || separator === trimmed.length - 1) return null;
  return {
    providerID: trimmed.slice(0, separator),
    modelID: trimmed.slice(separator + 1),
  };
}

export function parseServerUrlFromOutput(output: string): string | null {
  for (const line of output.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed.toLowerCase().includes("listening")) continue;
    const match = trimmed.match(/on\s+(https?:\/\/[^\s]+)/i);
    if (match?.[1]) return match[1].replace(/[.,;]+$/, "");
    if (trimmed.startsWith(OPENCODE_SERVER_READY_PREFIX)) {
      const fallback = trimmed.match(/(https?:\/\/[^\s]+)/i);
      if (fallback?.[1]) return fallback[1].replace(/[.,;]+$/, "");
    }
  }
  return null;
}

export function parseOpenCodeVersion(output: string): string | null {
  const match = output.match(/\d+\.\d+\.\d+/);
  return match?.[0] ?? null;
}

export function compareSemver(left: string, right: string): number {
  const a = left.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const b = right.split(".").map((part) => Number.parseInt(part, 10) || 0);
  for (let i = 0; i < 3; i += 1) {
    const delta = (a[i] ?? 0) - (b[i] ?? 0);
    if (delta !== 0) return delta;
  }
  return 0;
}

export function openCodeApiGeneration(
  version: string,
): OpenCodeApiGeneration | null {
  const major = Number.parseInt(version.split(".")[0] ?? "", 10);
  if (major === 1) {
    return compareSemver(version, MINIMUM_OPENCODE_VERSION) >= 0 ? "v1" : null;
  }
  if (major === 2) {
    return compareSemver(version, MINIMUM_OPENCODE_V2_VERSION) >= 0
      ? "v2"
      : null;
  }
  return null;
}

export function assertSupportedOpenCodeVersion(
  version: string | null,
): OpenCodeApiGeneration {
  if (!version) {
    throw new Error(
      `Unable to determine OpenCode version. MonoCode requires v${MINIMUM_OPENCODE_VERSION} or newer.`,
    );
  }
  const generation = openCodeApiGeneration(version);
  if (generation) return generation;
  const major = Number.parseInt(version.split(".")[0] ?? "", 10);
  if (major === 1) {
    throw new Error(
      `OpenCode v${version} is too old. Upgrade to v${MINIMUM_OPENCODE_VERSION} or newer.`,
    );
  }
  if (major === 2) {
    throw new Error(
      `OpenCode v${version} is too old. Upgrade to v${MINIMUM_OPENCODE_V2_VERSION} or newer.`,
    );
  }
  throw new Error(
    `OpenCode v${version} is not supported. MonoCode supports OpenCode v1 and v2.`,
  );
}

export function isOpenCodeDefaultTitle(title: string): boolean {
  return OPENCODE_DEFAULT_TITLE_PATTERN.test(title);
}

export function sameDirectory(left: string, right: string): boolean {
  const normalize = (value: string) =>
    value.replace(/\\/g, "/").replace(/\/+$/, "");
  return normalize(left) === normalize(right);
}

/** Inserts into an insertion-ordered map, dropping the oldest entry past `limit`. */
export function rememberBounded<K, V>(
  map: Map<K, V>,
  key: K,
  value: V,
  limit: number,
): void {
  map.set(key, value);
  dropOldest(map, limit);
}

export function rememberBoundedSet<K>(set: Set<K>, key: K, limit: number): void {
  set.add(key);
  dropOldest(set, limit);
}

function dropOldest<K>(collection: Map<K, unknown> | Set<K>, limit: number): void {
  if (collection.size <= limit) return;
  const oldest = collection.keys().next();
  if (!oldest.done) collection.delete(oldest.value);
}

export function isOpenCodeNotFound(cause: unknown): boolean {
  const seen = new Set<object>();
  const queue: unknown[] = [cause];
  for (let steps = 0; queue.length > 0 && steps < 32; steps += 1) {
    const node = queue.shift();
    if (node === null || typeof node !== "object" || seen.has(node)) continue;
    seen.add(node);
    const record = node as Record<string, unknown>;
    const response = record.response;
    const statuses = [
      record.status,
      record.statusCode,
      response !== null && typeof response === "object"
        ? (response as { status?: unknown }).status
        : undefined,
    ].filter((status): status is number => typeof status === "number");
    if (statuses.includes(404)) return true;
    if (statuses.length > 0) continue;
    const name = record.name;
    if (typeof name === "string" && name.toLowerCase() === "notfounderror") {
      return true;
    }
    for (const key of ["cause", "body", "error", "data"] as const) {
      if (record[key] !== undefined) queue.push(record[key]);
    }
  }
  return false;
}

export function parseOpenCodeToolOutputGlob(output: string): string {
  const paths = output.split(/\r?\n/).flatMap((line) => {
    const match = line.match(/^data[ \t]+(.+)$/);
    return match ? [match[1]] : [];
  });
  const path = paths[0];
  const windows =
    path != null &&
    (/^[A-Za-z]:[\\/]/.test(path) || /^\\\\[^\\/]+[\\/][^\\/]+/.test(path));
  if (
    paths.length !== 1 ||
    !path ||
    path.trim() !== path ||
    /[\x00-\x1f*?\[\]{}]/.test(path) ||
    !(path.startsWith("/") || windows) ||
    path
      .split(windows ? /[\\/]/ : /\//)
      .some((part) => part === "." || part === "..")
  )
    throw new Error(
      "OpenCode did not expose a safe data directory for its tool output.",
    );
  const separator = windows && path.includes("\\") ? "\\" : "/";
  const directory =
    separator === "\\" ? path.replace(/\\+$/, "") : path.replace(/\/+$/, "");
  return `${directory}${separator}tool-output${separator}*`;
}

/** The read-only permissions Plan mode allows. Match these names exactly:
 * a substring match would let a custom `spreadsheet_delete` tool through. */
export const OPENCODE_PLAN_READ_ONLY_PERMISSIONS: readonly string[] = [
  "read",
  "grep",
  "glob",
  "list",
  "websearch",
  "codesearch",
];

export function buildOpenCodePermissionRules(
  runtimeMode: RuntimeMode,
  planning = false,
  toolOutputGlob?: string,
): OpenCodePermissionRule[] {
  const outputRules: OpenCodePermissionRule[] = toolOutputGlob
    ? [
        {
          permission: "external_directory",
          pattern: toolOutputGlob,
          action: "allow",
        },
      ]
    : [];
  if (planning) {
    return [
      { permission: "*", pattern: "*", action: "deny" },
      ...[...OPENCODE_PLAN_READ_ONLY_PERMISSIONS, "question"].map(
        (permission): OpenCodePermissionRule => ({
          permission,
          pattern: "*",
          action: "allow",
        }),
      ),
      ...outputRules,
      { permission: "task", pattern: "explore", action: "allow" },
    ];
  }
  if (runtimeMode === "full-access") {
    return [{ permission: "*", pattern: "*", action: "allow" }];
  }
  const rules: OpenCodePermissionRule[] = [
    { permission: "*", pattern: "*", action: "ask" },
    { permission: "question", pattern: "*", action: "allow" },
    ...outputRules,
  ];
  if (runtimeMode === "auto-accept-edits" || runtimeMode === "auto") {
    rules.push({ permission: "edit", pattern: "*", action: "allow" });
  }
  if (runtimeMode === "auto") {
    rules.push({ permission: "read", pattern: "*", action: "allow" });
  }
  return rules;
}

/** Effective agent rules expose custom permission keys that must also be overridden. */
export function managedOpenCodeConfig(
  agentListOutput: string,
  runtimeMode: RuntimeMode,
  planning: boolean,
  toolOutputGlob?: string,
): Record<string, unknown> {
  const agents = new Map<string, Set<string>>();
  const externalPatterns = new Map<string, Set<string>>();
  const primaryAgents = new Set<string>();
  let name: string | undefined;
  let mode: string | undefined;
  let lines: string[] = [];
  const flush = () => {
    if (!name) return;
    let rules: unknown;
    try {
      rules = JSON.parse(lines.join("\n"));
    } catch {
      throw new Error(`Could not read OpenCode permissions for agent ${name}`);
    }
    if (!Array.isArray(rules))
      throw new Error(`OpenCode did not expose permissions for agent ${name}`);
    agents.set(
      name,
      new Set(
        rules.flatMap((rule) => {
          const permission = stringField(asRecord(rule), "permission");
          return permission ? [permission] : [];
        }),
      ),
    );
    externalPatterns.set(
      name,
      new Set(
        rules.flatMap((rule) => {
          const record = asRecord(rule);
          const pattern = stringField(record, "pattern");
          return record?.permission === "external_directory" &&
            pattern &&
            pattern !== "*"
            ? [pattern]
            : [];
        }),
      ),
    );
    if (mode === "primary") primaryAgents.add(name);
  };
  for (const line of agentListOutput.split("\n")) {
    const header = line.match(/^(.+)\s+\((primary|subagent|all)\)\s*$/);
    if (header) {
      flush();
      name = header[1];
      mode = header[2];
      lines = [];
    } else if (name) lines.push(line);
  }
  flush();
  if (agents.size === 0)
    throw new Error("OpenCode did not expose its agent permission rules");
  const actionFor = (permission: string): "allow" | "ask" | "deny" => {
    if (permission === "question") return "allow";
    if (planning)
      return OPENCODE_PLAN_READ_ONLY_PERMISSIONS.includes(permission)
        ? "allow"
        : "deny";
    if (runtimeMode === "full-access") return "allow";
    if (
      permission === "edit" &&
      ["auto", "auto-accept-edits"].includes(runtimeMode)
    )
      return "allow";
    if (permission === "read" && runtimeMode === "auto") return "allow";
    return "ask";
  };
  const permissions = (
    keys: Iterable<string>,
    patterns: Iterable<string>,
    allowExploreTask = false,
  ): Record<string, unknown> => {
    const policy: Record<string, unknown> = Object.fromEntries(
      [...keys].map((key) => [key, actionFor(key)]),
    );
    for (const rule of buildOpenCodePermissionRules(runtimeMode, planning)) {
      if (rule.pattern === "*") policy[rule.permission] = rule.action;
    }
    // TaskTool uses the target agent's own rules for child sessions. A scalar
    // deny replaces inherited scoped grants instead of leaving Explore enabled.
    if (planning)
      policy.task = allowExploreTask
        ? { "*": "deny", explore: "allow" }
        : "deny";
    if (planning || runtimeMode !== "full-access") {
      policy.external_directory = Object.fromEntries([
        ["*", actionFor("external_directory")],
        ...[...patterns].map((pattern) => [
          pattern,
          actionFor("external_directory"),
        ]),
        // Only the host's data directory can bypass the external-path baseline.
        ...(toolOutputGlob ? [[toolOutputGlob, "allow"]] : []),
      ]);
    }
    return policy;
  };
  return {
    permission: permissions(
      new Set([...agents.values()].flatMap((keys) => [...keys])),
      new Set(
        [...externalPatterns.values()].flatMap((patterns) => [...patterns]),
      ),
    ),
    agent: Object.fromEntries(
      [...agents].map(([agent, keys]) => [
        agent,
        {
          permission: permissions(
            keys,
            externalPatterns.get(agent)!,
            primaryAgents.has(agent) && agent !== "explore",
          ),
        },
      ]),
    ),
    mode: Object.fromEntries(
      [...primaryAgents].map((agent) => [
        agent,
        {
          permission: permissions(
            agents.get(agent)!,
            externalPatterns.get(agent)!,
            agent !== "explore",
          ),
        },
      ]),
    ),
    ...(planning || runtimeMode !== "full-access"
      ? { experimental: { primary_tools: [] } }
      : {}),
  };
}

export function verifyManagedOpenCodePolicy(
  agentsValue: unknown,
  configValue: unknown,
  runtimeMode: RuntimeMode,
  planning: boolean,
  toolOutputGlob?: string,
): void {
  if (!planning && runtimeMode === "full-access") return;
  const conflict = (): never => {
    throw new Error(
      "OpenCode configuration grants tools beyond the selected access mode. Remove conflicting agent permissions or experimental.primary_tools settings.",
    );
  };
  if (!asRecord(configValue)) conflict();
  const primaryTools = asRecord(
    asRecord(configValue)?.experimental,
  )?.primary_tools;
  if (
    primaryTools !== undefined &&
    (!Array.isArray(primaryTools) || primaryTools.length > 0)
  )
    conflict();
  if (!Array.isArray(agentsValue) || agentsValue.length === 0) conflict();
  for (const value of agentsValue as unknown[]) {
    const agent = asRecord(value);
    const permissions = agent?.permission;
    // TaskTool can select Explore by name even if config changes its mode.
    const allowExploreTask =
      agent?.mode === "primary" && agent?.name !== "explore";
    if (!Array.isArray(permissions)) conflict();
    const rules = (permissions as unknown[]).map(
      (value): OpenCodePermissionRule => {
        const rule = asRecord(value);
        const permission = stringField(rule, "permission");
        const pattern = stringField(rule, "pattern");
        const action = stringField(rule, "action");
        if (
          !permission ||
          !pattern ||
          !["allow", "ask", "deny"].includes(action ?? "")
        )
          conflict();
        return {
          permission: permission!,
          pattern: pattern!,
          action: action as OpenCodePermissionRule["action"],
        };
      },
    );
    const baseline = rules.reduce(
      (index, rule, current) =>
        rule.permission === "*" && rule.pattern === "*" ? current : index,
      -1,
    );
    if (
      baseline < 0 ||
      rules[baseline].action === "allow" ||
      (planning && rules[baseline].action !== "deny")
    )
      conflict();
    const later: OpenCodePermissionRule[] = [];
    for (const rule of rules.slice(baseline + 1).reverse()) {
      const covered = later.some(
        (override) =>
          (override.permission === "*" ||
            override.permission === rule.permission) &&
          (override.pattern === "*" || override.pattern === rule.pattern),
      );
      if (!covered && rule.action !== "deny") {
        const allowed =
          rule.permission === "question" ||
          (rule.permission === "external_directory" &&
            toolOutputGlob !== undefined &&
            rule.pattern === toolOutputGlob) ||
          (planning &&
            (OPENCODE_PLAN_READ_ONLY_PERMISSIONS.includes(rule.permission) ||
              (allowExploreTask &&
                rule.permission === "task" &&
                rule.pattern === "explore"))) ||
          (!planning &&
            ((rule.permission === "edit" &&
              ["auto", "auto-accept-edits"].includes(runtimeMode)) ||
              (rule.permission === "read" && runtimeMode === "auto")));
        if (!allowed && (planning || rule.action === "allow")) conflict();
      }
      later.push(rule);
    }
  }
}

export function toOpenCodeV2PermissionRules(
  rules: OpenCodePermissionRule[],
): OpenCodeV2PermissionRule[] {
  return rules.map((rule) => ({
    action: rule.permission,
    resource: rule.pattern,
    effect: rule.action,
  }));
}

export function toOpenCodePermissionReply(
  decision: "allow" | "deny",
): "once" | "reject" {
  return decision === "allow" ? "once" : "reject";
}

export function toFileUrl(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const abs = normalized.startsWith("/") ? normalized : `/${normalized}`;
  return `file://${abs.split("/").map(encodeURIComponent).join("/")}`;
}

export type OpenCodePromptPart =
  | { type: "text"; text: string }
  | { type: "file"; mime: string; filename: string; url: string };

/**
 * OpenCode forwards native file parts to the selected model provider. Keep
 * those parts to formats its provider adapters consistently support; local
 * files of every other type remain available to the agent through their path.
 */
export function toOpenCodePromptParts(
  text: string,
  attachments: Attachment[] | undefined,
): OpenCodePromptPart[] {
  const body = promptText(text, attachments ?? []);
  const textParts = body ? [body] : [];
  const parts: Array<{
    type: "file";
    mime: string;
    filename: string;
    url: string;
  }> = [];
  for (const attachment of attachments ?? []) {
    const mime = attachment.mimeType.trim().toLowerCase();
    if (!mime.startsWith("text/") && !isVisionImage(mime)) {
      textParts.push(attachmentPathText(attachment));
      continue;
    }
    const url =
      !attachment.path && attachment.data
        ? `data:${attachment.mimeType};base64,${attachment.data}`
        : toFileUrl(attachmentPath(attachment));
    parts.push({
      type: "file",
      mime: attachment.mimeType,
      filename: attachment.name,
      url,
    });
  }
  return [
    ...(textParts.length > 0
      ? [{ type: "text" as const, text: textParts.join("\n\n") }]
      : []),
    ...parts,
  ];
}

export function mergeOpenCodeAssistantText(
  previousText: string | undefined,
  nextText: string,
  final = false,
): { latestText: string; deltaToEmit: string } {
  const latestText =
    !final &&
    previousText &&
    previousText.length > nextText.length &&
    previousText.startsWith(nextText)
      ? previousText
      : nextText;
  return {
    latestText,
    deltaToEmit: latestText.slice(
      commonPrefixLength(previousText ?? "", latestText),
    ),
  };
}

export function appendOpenCodeAssistantTextDelta(
  previousText: string,
  delta: string,
): { nextText: string; deltaToEmit: string } {
  return { nextText: previousText + delta, deltaToEmit: delta };
}

function commonPrefixLength(left: string, right: string): number {
  let index = 0;
  while (
    index < left.length &&
    index < right.length &&
    left[index] === right[index]
  ) {
    index += 1;
  }
  return index;
}

export function titleCaseSlug(value: string): string {
  const segments: string[] = [];
  for (const segment of value.split(/[-_/]+/)) {
    if (segment.length > 0) {
      segments.push(segment.charAt(0).toUpperCase() + segment.slice(1));
    }
  }
  return segments.join(" ");
}

export function inferDefaultVariant(
  providerID: string,
  variants: string[],
): string | undefined {
  if (variants.length === 1) return variants[0];
  if (providerID === "anthropic" || providerID.startsWith("google")) {
    return variants.includes("high") ? "high" : undefined;
  }
  // Variants are reasoning levels on every provider (e.g. minimal/low/medium
  // /high/xhigh), so prefer medium, then high, regardless of provider.
  if (variants.includes("medium")) return "medium";
  if (variants.includes("high")) return "high";
  return undefined;
}

const VARIANT_LABELS: Record<string, string> = {
  none: "None",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  "extra-high": "Extra High",
  max: "Max",
  ultra: "Ultra",
};

const VARIANT_ORDER = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "extra-high",
  "max",
  "ultra",
];

/** Human label for an OpenCode variant value, matching Codex/Cursor effort labels. */
export function openCodeVariantLabel(value: string): string {
  return (
    VARIANT_LABELS[value] ??
    VARIANT_LABELS[value.toLowerCase()] ??
    titleCaseSlug(value)
  );
}

/** Sort variant values from lowest to highest effort; unknown values sort last. */
export function sortOpenCodeVariants(values: string[]): string[] {
  const rank = (value: string): number => {
    const index = VARIANT_ORDER.indexOf(value.toLowerCase());
    return index < 0 ? Number.MAX_SAFE_INTEGER : index;
  };
  return [...values].sort((left, right) => {
    const delta = rank(left) - rank(right);
    return delta !== 0 ? delta : left.localeCompare(right);
  });
}

export function inferDefaultAgent(
  agents: Array<{ name: string }>,
): string | undefined {
  return (
    agents.find((agent) => agent.name === "build")?.name ?? agents[0]?.name
  );
}

export function toolKindFromName(toolName: string): string {
  const normalized = toolName.toLowerCase();
  if (isTaskListToolName(toolName)) return "tasks";
  if (
    normalized.includes("bash") ||
    normalized.includes("command") ||
    normalized.includes("shell")
  ) {
    return "shell";
  }
  if (
    normalized.includes("edit") ||
    normalized.includes("write") ||
    normalized.includes("patch") ||
    normalized.includes("multiedit")
  ) {
    return "edit";
  }
  if (normalized.includes("read")) return "read";
  if (
    normalized.includes("grep") ||
    normalized.includes("glob") ||
    normalized.includes("search") ||
    normalized.includes("find") ||
    normalized === "list"
  ) {
    return "search";
  }
  if (normalized === "skill" || normalized === "skills") return "skill";
  if (
    normalized === "agent" ||
    normalized === "task" ||
    normalized === "subagent"
  ) {
    return "agent";
  }
  return toolName;
}

export function previewFromToolPart(
  part: OpenCodePart,
): ToolPreview | undefined {
  const tool = part.tool ?? "tool";
  const state = part.state ?? {};
  const kind = toolKindFromName(tool);
  return extractToolPreview(
    {
      title: typeof state.title === "string" ? state.title : tool,
      name: tool,
      kind,
      input: state.input,
      rawInput: state.input,
      content: state.output ?? state.metadata,
    },
    {
      title: tool,
      name: tool,
      kind,
      rawInput: state.input,
    },
  );
}

export function detailFromToolPart(part: OpenCodePart): string | undefined {
  const state = part.state ?? {};
  const status = typeof state.status === "string" ? state.status : "";
  if (status === "completed" && typeof state.output === "string")
    return state.output;
  if (status === "error") {
    if (typeof state.error === "string") return state.error;
    const error = asRecord(state.error);
    const data = asRecord(error?.data);
    return (
      stringField(data, "message") ??
      stringField(error, "message") ??
      stringField(asRecord(error?.error), "message")
    );
  }
  if (status === "running" && typeof state.title === "string")
    return state.title;
  return undefined;
}

export function permissionTitle(
  permission: string,
  patterns: string[],
): string {
  const detail = patterns.length > 0 ? patterns.join("\n") : permission;
  switch (permission) {
    case "bash":
      return detail ? `Run ${detail}` : "Run command";
    case "edit":
      return detail ? `Edit ${detail}` : "Edit file";
    case "read":
      return detail ? `Read ${detail}` : "Read file";
    default:
      return detail || permission;
  }
}

export function sessionErrorMessage(error: unknown): string {
  if (!error || typeof error !== "object") return "OpenCode session failed.";
  const rec = error as Record<string, unknown>;
  const data = asRecord(rec.data);
  const message =
    stringField(data, "message") ??
    stringField(rec, "message") ??
    stringField(asRecord(rec.error), "message");
  return message ?? "OpenCode session failed.";
}

/**
 * Context level from an OpenCode assistant message `info`.
 *
 * Cached reads still occupy the window, so they count alongside fresh input;
 * output counts because it carries into the next request.
 */
export function contextUsedFromMessageInfo(
  info: Record<string, unknown> | null,
): number | undefined {
  const tokens = asRecord(info?.tokens);
  if (!tokens) return undefined;
  const cache = asRecord(tokens.cache);
  const num = (rec: Record<string, unknown> | null, key: string): number => {
    const value = rec?.[key];
    return typeof value === "number" && Number.isFinite(value) ? value : 0;
  };
  const used =
    num(tokens, "input") +
    num(tokens, "output") +
    num(tokens, "reasoning") +
    num(cache, "read") +
    num(cache, "write");
  return used > 0 ? used : undefined;
}

export function turnMetricsFromMessageInfo(
  info: Record<string, unknown> | null,
): TurnMetrics | undefined {
  const tokens = asRecord(info?.tokens);
  if (!tokens) return undefined;
  const cache = asRecord(tokens.cache);
  const num = (rec: Record<string, unknown> | null, key: string): number => {
    const value = rec?.[key];
    return typeof value === "number" && Number.isFinite(value) ? value : 0;
  };
  const inputTokens = num(tokens, "input");
  const outputTokens = num(tokens, "output") + num(tokens, "reasoning");
  const cacheReadTokens = num(cache, "read");
  const cacheWriteTokens = num(cache, "write");
  const cacheReported = cache !== null;
  const cacheableInput = inputTokens + cacheReadTokens + cacheWriteTokens;
  if (!inputTokens && !outputTokens && !cacheableInput) return undefined;
  return {
    ...(inputTokens ? { inputTokens } : {}),
    ...(outputTokens ? { outputTokens } : {}),
    ...(cacheReadTokens ? { cacheReadTokens } : {}),
    ...(cacheWriteTokens ? { cacheWriteTokens } : {}),
    ...(cacheReported && cacheableInput
      ? { cacheHitPercent: (cacheReadTokens / cacheableInput) * 100 }
      : {}),
  };
}

/**
 * The session a `task` tool spawned, when OpenCode names it on the call. A
 * subagent runs as its own session, so this is what ties the child's stream
 * back to the row that started it.
 */
export function openCodeChildSessionId(part: OpenCodePart): string | undefined {
  const state = part.state ?? {};
  const metadata = asRecord(state.metadata);
  const input = asRecord(state.input);
  for (const source of [metadata, state, input]) {
    const id =
      stringField(source, "sessionID") ??
      stringField(source, "sessionId") ??
      stringField(source, "session_id") ??
      stringField(source, "childSessionID") ??
      stringField(source, "subSessionID");
    if (id) return id;
  }
  return undefined;
}

export function eventSessionId(
  event: Record<string, unknown>,
): string | undefined {
  const properties = asRecord(event.properties);
  if (!properties) return undefined;
  const sessionID = stringField(properties, "sessionID");
  if (sessionID) return sessionID;
  const form = asRecord(properties.form);
  const request = asRecord(properties.request);
  const info = asRecord(properties.info);
  return (
    stringField(form, "sessionID") ??
    stringField(request, "sessionID") ??
    stringField(info, "sessionID") ??
    stringField(asRecord(properties.part), "sessionID") ??
    (typeof event.type === "string" && event.type.startsWith("session.")
      ? stringField(info, "id")
      : undefined)
  );
}

export function normalizeOpenCodeV2Event(
  event: Record<string, unknown>,
): Record<string, unknown> {
  if ("properties" in event) return event;
  const data = asRecord(event.data);
  if (!data) return event;
  return { ...event, properties: data };
}

export function textDeltaEvent(
  part: OpenCodePart,
  text: string,
): HarnessEvent | null {
  if (!text) return null;
  if (part.type === "reasoning") return { type: "reasoning.delta", text };
  return { type: "message.delta", text };
}
