import type { AgentModel, ModelSetting } from "../models";

const EFFORT_TOKENS = ["low", "medium", "high"];

export function modelsFromAntigravityOutput(stdout: string): AgentModel[] {
  const rows: Array<{ id: string; name: string }> = [];
  for (const raw of stdout.split(/\r?\n/)) {
    const line = raw.replace(/\x1b\[[0-9;]*[A-Za-z]/g, "").trim();
    if (!line || line.toLowerCase().startsWith("fetching ")) continue;
    const match = /^(\S+)\s+(.+)$/.exec(line);
    if (!match) continue;
    const id = match[1].trim();
    const name = match[2].trim();
    if (!id || !name) continue;
    rows.push({ id, name });
  }
  return groupAntigravityModels(rows);
}

function groupAntigravityModels(
  rows: Array<{ id: string; name: string }>,
): AgentModel[] {
  const families = new Map<
    string,
    {
      name: string;
      variants: Array<{ id: string; name: string; effort?: string }>;
    }
  >();

  for (const row of rows) {
    const parsed = parseEffortSuffix(row.id);
    const family = families.get(parsed.base) ?? {
      name: stripEffortWords(row.name),
      variants: [],
    };
    if (row.id === parsed.base || family.variants.length === 0) {
      family.name = stripEffortWords(row.name);
    }
    family.variants.push({
      id: row.id,
      name: row.name,
      effort: parsed.effort,
    });
    families.set(parsed.base, family);
  }

  return [...families.entries()].map(([base, family]) => {
    const efforts = [
      ...new Set(
        family.variants
          .map((variant) => variant.effort)
          .filter((value): value is string => Boolean(value)),
      ),
    ];
    const canonical =
      family.variants.find((variant) => variant.id === base) ??
      family.variants.find((variant) => variant.effort === "high") ??
      family.variants[0];
    const settings: ModelSetting[] = [];
    if (efforts.length > 1) {
      settings.push({
        id: "effort",
        label: "Effort",
        kind: "select",
        value: canonical?.effort ?? efforts[0],
        options: efforts.map((value) => ({
          value,
          label: value[0].toUpperCase() + value.slice(1),
        })),
      });
    }
    return {
      id: `antigravity:${base}`,
      harness: "antigravity" as const,
      name: family.name,
      nativeId: canonical?.id ?? base,
      settings: settings.length > 0 ? settings : undefined,
    };
  });
}

function parseEffortSuffix(id: string): { base: string; effort?: string } {
  for (const token of EFFORT_TOKENS) {
    const suffix = `-${token}`;
    if (id.endsWith(suffix) && id.length > suffix.length) {
      return { base: id.slice(0, -suffix.length), effort: token };
    }
  }
  return { base: id };
}

function stripEffortWords(name: string): string {
  return name.replace(/\s*\((Low|Medium|High|Thinking)\)\s*$/i, "").trim();
}

export function nativeAntigravityModelId(
  nativeId: string,
  settings?: Record<string, string>,
): string {
  const effort = settings?.effort;
  if (!effort) return nativeId;
  const parsed = parseEffortSuffix(nativeId);
  return parsed.effort ? `${parsed.base}-${effort}` : nativeId;
}

export function eventsFromAntigravityLine(line: string): Array<
  | { type: "session.providerBound"; providerSessionId: string }
  | { type: "message.delta"; text: string }
  | { type: "message.completed" }
  | { type: "session.error"; message: string }
> {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return [];
  let rec: Record<string, unknown>;
  try {
    rec = JSON.parse(trimmed) as Record<string, unknown>;
  } catch {
    return [];
  }
  const event = typeof rec.event === "string" ? rec.event : "";
  if (event === "init") {
    const init = asRecord(rec.init);
    const id =
      (typeof rec.conversation_id === "string" && rec.conversation_id) ||
      (typeof init?.conversation_id === "string" && init.conversation_id) ||
      "";
    return id
      ? [{ type: "session.providerBound", providerSessionId: id }]
      : [];
  }
  if (event === "step_update") {
    const update = asRecord(rec.step_update);
    const text =
      typeof update?.text_delta === "string" ? update.text_delta : "";
    return text ? [{ type: "message.delta", text }] : [];
  }
  if (event === "result") {
    const result = asRecord(rec.result);
    const status = String(result?.status ?? rec.status ?? "").toUpperCase();
    const response =
      typeof result?.response === "string" ? result.response : "";
    if (status && status !== "SUCCESS") {
      return [
        {
          type: "session.error",
          message: response || status,
        },
      ];
    }
    return [{ type: "message.completed" }];
  }
  return [];
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}
