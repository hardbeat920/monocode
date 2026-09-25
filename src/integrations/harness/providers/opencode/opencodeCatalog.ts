import { homeDir } from "../../../../platform/tauri/fs";
import {
  setHarnessModels,
  type AgentModel,
  type ModelSetting,
  type ModelSettingChoice,
} from "../../../../features/sessions/model/models";
import { execChild, resolveOpenCodeBinary } from "../../core/child";
import { OpenCodeClient } from "./opencodeClient";
import { resolveOpenCodeV2Service } from "./opencodeService";
import {
  asRecord,
  assertSupportedOpenCodeVersion,
  inferDefaultAgent,
  inferDefaultVariant,
  KNOWN_HIDDEN_AGENTS,
  openCodeVariantLabel,
  parseOpenCodeVersion,
  sortOpenCodeVariants,
  stringField,
  titleCaseSlug,
} from "./opencodeProtocol";

const SLUG_LINE_RE = /^(\S+\/\S+)\s*$/;
const AGENT_HEADER_RE = /^(.+)\s+\((\S+)\)\s*$/;

type OpenCodeModelJson = {
  id?: string;
  name?: string;
  variants?: Record<string, unknown>;
  limit?: { context?: number; input?: number; output?: number };
};

type ParsedProvider = {
  id: string;
  name: string;
  models: Record<string, OpenCodeModelJson>;
};

const PROVIDER_NAMES: Record<string, string> = {
  opencode: "OpenCode",
  "opencode-go": "OpenCode Go",
  openai: "OpenAI",
  xai: "xAI",
  "github-copilot": "GitHub Copilot",
};

export type OpenCodeAgent = {
  name: string;
  mode: string;
  hidden: boolean;
};

let inflight: Promise<void> | null = null;

export function refreshOpenCodeCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverOpenCodeModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("opencode", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] opencode catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverOpenCodeModels(): Promise<AgentModel[]> {
  const { path } = await resolveOpenCodeBinary();
  const cwd = await homeDir();
  const versionOut = await execChild(path, ["--version"], cwd);
  const version = parseOpenCodeVersion(versionOut);
  const generation = assertSupportedOpenCodeVersion(version);
  if (generation === "v2") {
    return discoverOpenCodeV2Models(path, cwd);
  }

  const modelsOut = await execChild(path, ["models", "--verbose"], cwd);
  const parsed = parseModelsCliOutput(modelsOut);
  let agents: OpenCodeAgent[] = [];
  try {
    const agentsOut = await execChild(path, ["agent", "list"], cwd);
    agents = parseAgentListCliOutput(agentsOut);
  } catch (error) {
    console.debug("[monocode] opencode agents", error);
  }
  return flattenOpenCodeModels(parsed, agents);
}

async function discoverOpenCodeV2Models(
  path: string,
  cwd: string,
): Promise<AgentModel[]> {
  const service = await resolveOpenCodeV2Service(path, cwd);
  // Catalog requests stay on the server's default location: location-scoped
  // requests return an empty model snapshot for some projects on 2.x and a
  // 500 for directories the server has not registered.
  const client = new OpenCodeClient(service.url, "", "v2", service.password);
  const [rawModels, rawProviders, rawAgents] = await Promise.all([
    client.listModels(),
    client.listProviders(),
    // Agent discovery is optional, as in the v1 path: its failure must not
    // lose models and providers.
    client.listAgents().catch(() => []),
  ]);
  return flattenOpenCodeModels(
    parseV2Catalog(rawModels, rawProviders),
    parseV2Agents(rawAgents),
  );
}

export function parseV2Catalog(
  rawModels: unknown[],
  rawProviders: unknown[],
): {
  providers: Map<string, ParsedProvider>;
  connected: string[];
} {
  const providerNames = new Map<string, string>();
  for (const value of rawProviders) {
    const provider = asRecord(value);
    const id = stringField(provider, "id");
    if (id) {
      providerNames.set(
        id,
        stringField(provider, "name") ?? openCodeProviderName(id),
      );
    }
  }
  const providers = new Map<string, ParsedProvider>();
  for (const value of rawModels) {
    const model = asRecord(value);
    if (!model) continue;
    const providerID = stringField(model, "providerID");
    // `id` is the selectable name and can be an alias that differs from the
    // upstream `modelID` (e.g. id "openai/coding", modelID "gpt-5.2").
    const rawId = stringField(model, "id") ?? stringField(model, "modelID");
    const modelID =
      providerID && rawId?.startsWith(`${providerID}/`)
        ? rawId.slice(providerID.length + 1)
        : rawId;
    if (!providerID || !modelID || model?.enabled === false) continue;
    let provider = providers.get(providerID);
    if (!provider) {
      provider = {
        id: providerID,
        name: providerNames.get(providerID) ?? openCodeProviderName(providerID),
        models: {},
      };
      providers.set(providerID, provider);
    }
    const variants = Array.isArray(model.variants)
      ? Object.fromEntries(
          model.variants.flatMap((variant) => {
            const id = stringField(asRecord(variant), "id");
            return id ? [[id, variant]] : [];
          }),
        )
      : (asRecord(model.variants) ?? {});
    const limit = asRecord(model.limit);
    provider.models[modelID] = {
      id: modelID,
      name: stringField(model, "name"),
      variants,
      limit: limit
        ? {
            context:
              typeof limit.context === "number" ? limit.context : undefined,
            input: typeof limit.input === "number" ? limit.input : undefined,
            output: typeof limit.output === "number" ? limit.output : undefined,
          }
        : undefined,
    };
  }
  return { providers, connected: [...providers.keys()] };
}

export function parseV2Agents(rawAgents: unknown[]): OpenCodeAgent[] {
  return rawAgents.flatMap((value) => {
    const agent = asRecord(value);
    // v2 sessions resolve agents by `id` ("build"); `name` is a display label ("Build").
    const name = stringField(agent, "id") ?? stringField(agent, "name");
    if (!name) return [];
    return [
      {
        name,
        mode: stringField(agent, "mode") ?? "all",
        hidden: agent?.hidden === true || KNOWN_HIDDEN_AGENTS.has(name),
      },
    ];
  });
}

export function parseModelsCliOutput(stdout: string): {
  providers: Map<string, ParsedProvider>;
  connected: string[];
} {
  const providers = new Map<string, ParsedProvider>();
  const lines = stdout.split("\n");
  let currentSlug: string | null = null;
  const jsonLines: string[] = [];

  const flushModel = () => {
    if (currentSlug === null || jsonLines.length === 0) {
      currentSlug = null;
      jsonLines.length = 0;
      return;
    }
    const jsonStr = jsonLines.join("\n").trim();
    if (jsonStr.length > 0) {
      try {
        const model = JSON.parse(jsonStr) as OpenCodeModelJson;
        const separator = currentSlug.indexOf("/");
        if (separator > 0) {
          const providerID = currentSlug.slice(0, separator);
          const modelID = currentSlug.slice(separator + 1);
          let provider = providers.get(providerID);
          if (!provider) {
            provider = {
              id: providerID,
              name: openCodeProviderName(providerID),
              models: {},
            };
            providers.set(providerID, provider);
          }
          provider.models[modelID] = model;
        }
      } catch {
        // Skip unparseable model JSON
      }
    }
    currentSlug = null;
    jsonLines.length = 0;
  };

  for (const line of lines) {
    const slugMatch = line.trimStart().startsWith("{")
      ? null
      : SLUG_LINE_RE.exec(line);
    if (slugMatch) {
      flushModel();
      currentSlug = slugMatch[1]!;
    } else if (currentSlug !== null) {
      jsonLines.push(line);
    }
  }
  flushModel();
  return { providers, connected: [...providers.keys()] };
}

export function parseAgentListCliOutput(stdout: string): OpenCodeAgent[] {
  const agents: OpenCodeAgent[] = [];
  const lines = stdout.split("\n");
  let currentHeader: { name: string; mode: string } | null = null;
  const blockLines: string[] = [];

  const flushAgent = () => {
    if (currentHeader === null) {
      currentHeader = null;
      blockLines.length = 0;
      return;
    }
    agents.push({
      name: currentHeader.name,
      mode: currentHeader.mode,
      hidden: KNOWN_HIDDEN_AGENTS.has(currentHeader.name),
    });
    currentHeader = null;
    blockLines.length = 0;
  };

  for (const line of lines) {
    const match = AGENT_HEADER_RE.exec(line);
    if (match) {
      flushAgent();
      currentHeader = { name: match[1]!, mode: match[2]! };
    } else if (currentHeader !== null) {
      blockLines.push(line);
    }
  }
  flushAgent();
  return agents;
}

export function flattenOpenCodeModels(
  parsed: { providers: Map<string, ParsedProvider>; connected: string[] },
  agents: OpenCodeAgent[],
): AgentModel[] {
  const connected = new Set(parsed.connected);
  const primaryAgents = agents.filter(
    (agent) =>
      !agent.hidden && (agent.mode === "primary" || agent.mode === "all"),
  );
  const models: AgentModel[] = [];
  for (const provider of parsed.providers.values()) {
    if (!connected.has(provider.id)) continue;
    for (const [modelId, model] of Object.entries(provider.models)) {
      const name = model.name?.trim() || titleCaseSlug(modelId);
      const nativeId = `${provider.id}/${model.id ?? modelId}`;
      const contextWindow = model.limit?.context;
      models.push({
        id: `opencode:${nativeId}`,
        harness: "opencode",
        name,
        nativeId,
        provider: { id: provider.id, name: provider.name },
        settings: openCodeModelSettings(provider.id, model, primaryAgents),
        ...(contextWindow && contextWindow > 0 ? { contextWindow } : {}),
      });
    }
  }
  return models.sort((left, right) => left.name.localeCompare(right.name));
}

export function openCodeProviderName(providerID: string): string {
  return PROVIDER_NAMES[providerID] ?? titleCaseSlug(providerID);
}

function openCodeModelSettings(
  providerID: string,
  model: OpenCodeModelJson,
  agents: OpenCodeAgent[],
): ModelSetting[] | undefined {
  const settings: ModelSetting[] = [];
  const variantValues = sortOpenCodeVariants(Object.keys(model.variants ?? {}));
  if (variantValues.length > 0) {
    const defaultVariant = inferDefaultVariant(providerID, variantValues);
    const options: ModelSettingChoice[] = variantValues.map((value) => ({
      value,
      label: openCodeVariantLabel(value),
    }));
    settings.push({
      id: "variant",
      label: "Variant",
      kind: "select",
      value: defaultVariant ?? options[0].value,
      options,
    });
  }
  if (agents.length > 0) {
    const defaultAgent = inferDefaultAgent(agents);
    settings.push({
      id: "agent",
      label: "Agent",
      kind: "select",
      value: defaultAgent ?? agents[0].name,
      options: agents.map((agent) => ({
        value: agent.name,
        label: titleCaseSlug(agent.name),
      })),
    });
  }
  return settings.length > 0 ? settings : undefined;
}
