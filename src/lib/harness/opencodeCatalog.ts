import { homeDir } from "../fs";
import {
  MODELS,
  setHarnessModels,
  type AgentModel,
  type ModelSetting,
  type ModelSettingChoice,
} from "../models";
import { execChild, resolveOpenCodeBinary } from "./child";
import {
  compareSemver,
  inferDefaultAgent,
  inferDefaultVariant,
  KNOWN_HIDDEN_AGENTS,
  MINIMUM_OPENCODE_VERSION,
  parseOpenCodeVersion,
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
      const byHarness = new Map<AgentModel["harness"], AgentModel[]>();
      for (const model of models) {
        const entries = byHarness.get(model.harness) ?? [];
        entries.push(model);
        byHarness.set(model.harness, entries);
      }
      for (const [harness, entries] of byHarness) {
        if (entries.length > 0) setHarnessModels(harness, entries);
      }
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
  if (!version) {
    throw new Error(
      `Unable to determine OpenCode version. MonoCode requires v${MINIMUM_OPENCODE_VERSION} or newer.`,
    );
  }
  if (compareSemver(version, MINIMUM_OPENCODE_VERSION) < 0) {
    throw new Error(
      `OpenCode v${version} is too old. Upgrade to v${MINIMUM_OPENCODE_VERSION} or newer.`,
    );
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
            provider = { id: providerID, name: providerID, models: {} };
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
    (agent) => !agent.hidden && (agent.mode === "primary" || agent.mode === "all"),
  );
  const models: AgentModel[] = [];
  for (const provider of parsed.providers.values()) {
    if (!connected.has(provider.id)) continue;
    for (const [modelId, model] of Object.entries(provider.models)) {
      const name = model.name?.trim() || titleCaseSlug(modelId);
      const nativeId = `${provider.id}/${model.id ?? modelId}`;
      const harness = providerHarness(provider.id, model.id ?? modelId);
      const contextWindow = model.limit?.context;
      models.push({
        id: `${harness}:${nativeId}`,
        harness,
        name,
        nativeId,
        settings: openCodeModelSettings(provider.id, model, primaryAgents),
        ...(contextWindow && contextWindow > 0 ? { contextWindow } : {}),
      });
    }
  }
  // Keep the providers MonoCode knows about even when the local OpenCode
  // installation has not emitted them yet. Once configured, the live catalog
  // entries above replace these fallback entries by their complete metadata.
  const seen = new Set(models.map((model) => model.nativeId));
  for (const model of MODELS) {
    if (
      !["opencode", "zai", "mimo", "openrouter", "nvidia"].includes(model.harness) ||
      !model.nativeId ||
      seen.has(model.nativeId)
    ) {
      continue;
    }
    models.push(model);
  }
  return models.sort((left, right) => left.name.localeCompare(right.name));
}

function providerHarness(
  providerID: string,
  modelID: string,
): AgentModel["harness"] {
  if (providerID === "zai" || (providerID === "opencode-go" && modelID.startsWith("glm-"))) {
    return "zai";
  }
  if (providerID === "mimo" || (providerID === "opencode-go" && modelID.startsWith("mimo-"))) {
    return "mimo";
  }
  if (providerID === "openrouter") return "openrouter";
  if (providerID === "nvidia") return "nvidia";
  return "opencode";
}

function openCodeModelSettings(
  providerID: string,
  model: OpenCodeModelJson,
  agents: OpenCodeAgent[],
): ModelSetting[] | undefined {
  const settings: ModelSetting[] = [];
  const variantValues = Object.keys(model.variants ?? {});
  if (variantValues.length > 0) {
    const defaultVariant = inferDefaultVariant(providerID, variantValues);
    const options: ModelSettingChoice[] = variantValues.map((value) => ({
      value,
      label: titleCaseSlug(value),
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
