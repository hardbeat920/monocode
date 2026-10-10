import type { AgentModel } from "../../../../features/sessions/model/models";
import { version } from "../../../../../package.json";
import {
  nativeCommandInvocation,
  type NativeCommand,
} from "../../core/nativeCommands";
import { asRecord } from "../grok/grokProtocol";

export const COPILOT_AUTH_HELP =
  "Run `copilot login` in Terminal or sign in from Providers.";

export const COPILOT_CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

export const COPILOT_INITIALIZE_PARAMS = {
  protocolVersion: 1,
  clientCapabilities: COPILOT_CLIENT_CAPABILITIES,
  clientInfo: { name: "monocode", version },
};

export function copilotSpawnArgs(): string[] {
  return ["--acp", "--stdio", "--no-auto-update"];
}

type ConfigOption = {
  id: string;
  currentValue?: unknown;
  options?: { value?: unknown }[];
};

export function copilotConfigOptions(result: unknown): ConfigOption[] {
  const options = asRecord(result)?.configOptions;
  if (!Array.isArray(options)) return [];
  return options.flatMap((value) => {
    const option = asRecord(value);
    if (typeof option?.id !== "string") return [];
    return [
      {
        id: option.id,
        currentValue: option.currentValue,
        options: Array.isArray(option.options)
          ? option.options.flatMap((entry) => {
              const value = asRecord(entry);
              return value ? [{ value: value.value }] : [];
            })
          : undefined,
      },
    ];
  });
}

/** Cache the URI form, including when a CLI reports shorthand mode ids. */
export function copilotModeId(value: string): string {
  return ["agent", "plan", "autopilot"].includes(value)
    ? `https://agentclientprotocol.com/protocol/session-modes#${value}`
    : value;
}

export function copilotError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  return new Error(
    /authentication required|not authenticated|unauthorized|not logged in/i.test(
      detail,
    )
      ? `${detail.trim()}\n\n${COPILOT_AUTH_HELP}`
      : detail,
  );
}

export function modelsFromCopilotSession(result: unknown): AgentModel[] {
  const state = asRecord(asRecord(result)?.models);
  const available = state?.availableModels;
  if (!Array.isArray(available)) return [];
  const seen = new Set<string>();
  const models: AgentModel[] = [];
  for (const value of available) {
    const model = asRecord(value);
    const id = model?.modelId;
    if (typeof id !== "string" || !id || seen.has(id)) continue;
    seen.add(id);
    models.push({
      id: `copilot:${id}`,
      harness: "copilot",
      nativeId: id,
      name: typeof model?.name === "string" ? model.name : id,
    });
  }
  const current = models.findIndex(
    (model) => model.nativeId === state?.currentModelId,
  );
  if (current > 0) models.unshift(...models.splice(current, 1));
  return models;
}

/** ACP option ids are opaque; select by their standardized kind. */
export function copilotPermissionOption(
  params: unknown,
  allow: boolean,
): string | undefined {
  const options = asRecord(params)?.options;
  if (!Array.isArray(options)) return undefined;
  for (const kind of allow
    ? ["allow_once", "allow_always"]
    : ["reject_once", "reject_always"]) {
    const option = options.map(asRecord).find((entry) => entry?.kind === kind);
    if (typeof option?.optionId === "string") return option.optionId;
  }
  return undefined;
}

export function commandsFromCopilotUpdate(
  params: unknown,
): NativeCommand[] | undefined {
  const update = asRecord(asRecord(params)?.update);
  if (update?.sessionUpdate !== "available_commands_update") return undefined;
  if (!Array.isArray(update.availableCommands)) return [];
  return update.availableCommands.flatMap((value) => {
    const command = asRecord(value);
    if (typeof command?.name !== "string" || !command.name) return [];
    const input = asRecord(command.input);
    return [
      {
        name: command.name,
        description:
          typeof command.description === "string" ? command.description : "",
        invocation: nativeCommandInvocation("copilot", command.name),
        source: "copilot" as const,
        inputHint: typeof input?.hint === "string" ? input.hint : undefined,
      },
    ];
  });
}
