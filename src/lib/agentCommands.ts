import codexCommands from "./codexCliCommands.json";
import claudeCommands from "./claudeCliCommands.json";
import { modelsFor, nativeModelId, type AgentModel } from "./models";
import {
  RUNTIME_MODES,
  type HarnessId,
  type RuntimeMode,
  type Session,
} from "./session";
import type { Skill } from "./skills";

export type SlashFilter = "all" | "commands" | "skills";
const FILTER_KEY = "monocode.slashFilter";

export function supportsAgentCommands(harness: HarnessId): boolean {
  return harness === "codex" || harness === "claude";
}

export const CHAT_COMMANDS: Record<string, string> = {
  status: "Show this chat's model, permissions and context usage.",
  context: "Show context usage reported for this chat.",
  model: "Choose the model for this chat: /model [model ID].",
  permissions: "Choose this chat's access mode: /permissions [mode].",
  approvals: "Choose this chat's access mode.",
  compact:
    "Compact the current conversation using its existing agent connection.",
  plan: "Plan the next message in this chat: /plan [request].",
  fast: "Configure fast mode for the current model: /fast [on|off].",
  effort: "Configure the current model's reasoning effort: /effort [level].",
  reasoning:
    "Configure the current model's reasoning effort: /reasoning [level].",
  diff: "Open this chat's project diff in MonoCode.",
  stop: "Stop the current agent turn.",
  help: "Show commands supported in this chat.",
};

// Documented CLI names are also recognized so unsupported commands never
// silently launch a terminal or become model prompts in Agent commands mode.
export function agentCommands(harness: HarnessId): Skill[] {
  if (!supportsAgentCommands(harness)) return [];
  const names = harness === "codex" ? codexCommands : claudeCommands;
  return [...new Set([...Object.keys(CHAT_COMMANDS), ...names])].map(
    (name) => ({
      kind: "native",
      name,
      invocation: `agent:${name}`,
      aliases: [name, `cli:${name}`],
      description: CHAT_COMMANDS[name] ?? "Not supported in MonoCode chat yet.",
      source: harness,
      origin: CHAT_COMMANDS[name] ? "Chat" : "Not supported in chat",
    }),
  );
}

export function loadSlashFilter(): SlashFilter {
  try {
    const value = localStorage.getItem(FILTER_KEY);
    return value === "commands" || value === "skills" ? value : "all";
  } catch {
    return "all";
  }
}

export function saveSlashFilter(filter: SlashFilter): void {
  try {
    localStorage.setItem(FILTER_KEY, filter);
  } catch {
    /* private mode */
  }
}

export function filterSlashItems(items: Skill[], filter: SlashFilter): Skill[] {
  return items.filter(
    (item) =>
      filter === "all" ||
      (filter === "commands"
        ? item.kind === "native"
        : item.kind === "file" || item.name === "create-skill"),
  );
}

/** Preserve skills and MonoCode shortcuts. /agent:name disambiguates them;
 * /cli:name from the earlier local build is a compatibility alias only. */
export function agentCommandPrompt(
  text: string,
  harness: HarnessId,
  filter: SlashFilter,
  skills: Skill[],
): string | null {
  if (!supportsAgentCommands(harness)) return null;
  const match = text.match(/^\s*\/([a-zA-Z0-9_.:-]+)(?=\s|$)/);
  if (!match) return null;
  const name = match[1];
  if (/^(agent|cli):.+/.test(name))
    return text.trimStart().replace(/^\/(agent|cli):/, "/");
  if (filter === "skills") return null;
  if (filter === "commands") return text.trimStart();
  if (
    name === "plan" ||
    name === "compact" ||
    skills.some((skill) => skill.invocation === name)
  )
    return null;
  return agentCommands(harness).some((item) => item.name === name)
    ? text.trimStart()
    : null;
}

export type CommandPanel = {
  command: string;
  kind:
    | "status"
    | "context"
    | "model"
    | "permissions"
    | "settings"
    | "help"
    | "notice";
  message?: string;
  error?: boolean;
};

type CommandHandlers = {
  onModelChange: (id: string, harness: HarnessId, model: string) => void;
  onModelSettingsChange: (id: string, values: Record<string, string>) => void;
  onRuntimeModeChange: (id: string, mode: RuntimeMode) => void;
  onCompactContext: (id: string) => boolean;
  onStop: (id: string) => void;
  onOpenDiff: (
    path?: string,
    session?: { sessionId: string; cwd: string },
  ) => void;
};

export function commandsLocked(session: Session): boolean {
  return !!session.busy || !!session.queuedMessages?.length;
}

export function commandModel(
  session: Pick<Session, "harness" | "model">,
): AgentModel | undefined {
  return modelsFor(session.harness).find((model) => model.id === session.model);
}

/** Dispatch against the same MonoCode session callbacks used by its controls.
 * No PTY, fork, shell, second conversation or generic model-turn fallback. */
export function runChatCommand(
  session: Session,
  text: string,
  handlers: CommandHandlers,
): { accepted: boolean; panel: CommandPanel } {
  const match = text.trim().match(/^\/([a-zA-Z0-9_.:-]+)(?: +(.+))?$/);
  const name = match?.[1] ?? "command";
  const argument = match?.[2]?.trim() ?? "";
  const result = (kind: CommandPanel["kind"], message?: string) => ({
    accepted: true,
    panel: { command: name, kind, message },
  });
  const reject = (message: string) => ({
    accepted: false,
    panel: { command: name, kind: "notice" as const, message, error: true },
  });
  if (!match || /[\x00-\x1f\x7f]/.test(text))
    return reject(
      "Enter one slash command without line breaks or control characters.",
    );
  if (!supportsAgentCommands(session.harness) || !CHAT_COMMANDS[name])
    return reject(
      `/${name} is not supported in MonoCode chat yet. Use /help to see supported commands.`,
    );
  if (
    ![
      "model",
      "permissions",
      "approvals",
      "fast",
      "effort",
      "reasoning",
    ].includes(name) &&
    argument
  )
    return reject(`/${name} does not accept arguments in MonoCode chat.`);
  if (
    [
      "model",
      "permissions",
      "approvals",
      "fast",
      "effort",
      "reasoning",
      "compact",
    ].includes(name) &&
    commandsLocked(session)
  )
    return reject(
      "Finish the current turn and queued messages before changing this chat's settings or compacting it.",
    );
  switch (name) {
    case "status":
    case "context":
    case "help":
      return result(name);
    case "model": {
      if (argument) {
        const query = argument.toLowerCase();
        const model = modelsFor(session.harness).find((entry) =>
          [entry.id, entry.name, nativeModelId(entry)].some(
            (value) => value.toLowerCase() === query,
          ),
        );
        if (!model)
          return reject(
            `Unknown model: ${argument}. Use /model and select a model from this agent's catalog.`,
          );
        handlers.onModelChange(session.id, session.harness, model.id);
      }
      return result("model");
    }
    case "permissions":
    case "approvals": {
      if (argument) {
        const mode = RUNTIME_MODES.find((value) => value === argument);
        if (!mode) return reject(`Choose one of: ${RUNTIME_MODES.join(", ")}.`);
        handlers.onRuntimeModeChange(session.id, mode);
      }
      return result("permissions");
    }
    case "fast":
    case "effort":
    case "reasoning": {
      const model = commandModel(session);
      const setting = model?.settings?.find((entry) =>
        name === "fast"
          ? entry.id === "fast"
          : ["effort", "reasoning"].includes(entry.id),
      );
      if (!setting)
        return reject(
          `The current model's catalog does not expose a ${name} setting.`,
        );
      if (argument) {
        const value =
          argument === "on" ? "true" : argument === "off" ? "false" : argument;
        const allowed =
          setting.kind === "toggle"
            ? ["true", "false"]
            : setting.options.map((option) => option.value);
        if (!allowed.includes(value))
          return reject(`Choose one of: ${allowed.join(", ")}.`);
        handlers.onModelSettingsChange(session.id, {
          ...session.modelSettings,
          [setting.id]: value,
        });
      }
      return result("settings");
    }
    case "compact":
      return handlers.onCompactContext(session.id)
        ? result(
            "notice",
            "Compacting this conversation. Progress appears in the chat.",
          )
        : reject("This conversation cannot be compacted right now.");
    case "stop":
      handlers.onStop(session.id);
      return result(
        "notice",
        session.busy ? "Stopping this turn." : "No turn is running.",
      );
    case "diff":
      handlers.onOpenDiff(undefined, {
        sessionId: session.id,
        cwd: session.worktreeCwd || session.cwd,
      });
      return result("notice", "Opened this project's diff.");
    default:
      return reject("Use /plan in the composer to plan your next message.");
  }
}
