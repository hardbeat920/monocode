// Adapted from T3 Code's custom model helpers. See NOTICE for attribution.
import type { AgentModel, ModelSetting } from "./models";
import type { HarnessId } from "./session";

export const CUSTOM_MODEL_HARNESSES = ["claude", "codex"] as const;
export type CustomModelHarness = (typeof CUSTOM_MODEL_HARNESSES)[number];
export const MAX_CUSTOM_MODEL_COUNT = 32;
export const MAX_CUSTOM_MODEL_LENGTH = 256;

export type CustomModelDefinition = {
  slug: string;
  name: string;
  /** null uses the provider's default options. */
  settings: ModelSetting[] | null;
};

type CustomModelSetting =
  string | { slug: string; name?: string; settings?: ModelSetting[] };

export function supportsCustomModels(
  harness: HarnessId,
): harness is CustomModelHarness {
  return harness === "claude" || harness === "codex";
}

/** Provider-owned identifiers are only trimmed, never expanded as aliases. */
export function normalizeCustomModelSlug(
  model: string | null | undefined,
): string | null {
  return typeof model === "string" ? model.trim() || null : null;
}

/** Keep custom slugs separate from MonoCode's shortened built-in ids. */
export function customModelId(
  harness: CustomModelHarness,
  slug: string,
): string {
  return `${harness}:custom:${slug}`;
}

export function customModelSlug(id: string): string | null {
  for (const harness of CUSTOM_MODEL_HARNESSES) {
    const prefix = `${harness}:custom:`;
    if (id.startsWith(prefix)) return id.slice(prefix.length);
  }
  return null;
}

export function readCustomModelEntries(
  value: unknown,
): CustomModelDefinition[] {
  if (!Array.isArray(value)) return [];
  const entries: CustomModelDefinition[] = [];
  const seen = new Set<string>();
  for (const raw of value) {
    const record = typeof raw === "string" ? { slug: raw } : asRecord(raw);
    if (!record) continue;
    const slug = normalizeCustomModelSlug(
      typeof record.slug === "string" ? record.slug : null,
    );
    if (!slug || slug.length > MAX_CUSTOM_MODEL_LENGTH || seen.has(slug))
      continue;
    seen.add(slug);
    const name =
      normalizeCustomModelSlug(
        typeof record.name === "string" ? record.name : null,
      ) ?? slug;
    entries.push({ slug, name, settings: readModelSettings(record.settings) });
    if (entries.length >= MAX_CUSTOM_MODEL_COUNT) break;
  }
  return entries;
}

export function toCustomModelSetting(
  entry: CustomModelDefinition,
): CustomModelSetting {
  const settings = entry.settings ?? [];
  const name = entry.name !== entry.slug ? entry.name : undefined;
  if (!name && settings.length === 0) return entry.slug;
  return {
    slug: entry.slug,
    ...(name ? { name } : {}),
    ...(settings.length > 0 ? { settings } : {}),
  };
}

/** Discovered models win; bare Codex entries inherit the catalog's options. */
export function appendCustomModels(
  harness: CustomModelHarness,
  models: AgentModel[],
  entries: CustomModelDefinition[],
): AgentModel[] {
  if (entries.length === 0) return models;
  const seen = new Set(
    models.map((model) => model.nativeId ?? model.id.slice(harness.length + 1)),
  );
  const fallbackSettings =
    harness === "codex"
      ? models.find((model) => model.settings?.length)?.settings
      : undefined;
  const custom: AgentModel[] = [];
  for (const entry of entries) {
    if (seen.has(entry.slug)) continue;
    seen.add(entry.slug);
    const settings = entry.settings ?? fallbackSettings;
    custom.push({
      id: customModelId(harness, entry.slug),
      harness,
      nativeId: entry.slug,
      name: entry.name,
      isCustom: true,
      ...(settings?.length ? { settings } : {}),
    });
  }
  return custom.length ? [...models, ...custom] : models;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** Invalid options do not make an otherwise usable model disappear. */
function readModelSettings(value: unknown): ModelSetting[] | null {
  if (!Array.isArray(value) || value.length === 0) return null;
  const settings: ModelSetting[] = [];
  const seen = new Set<string>();
  for (const raw of value) {
    const row = asRecord(raw);
    if (
      !row ||
      typeof row.id !== "string" ||
      !row.id.trim() ||
      typeof row.label !== "string" ||
      !row.label.trim() ||
      typeof row.value !== "string" ||
      (row.kind !== "select" && row.kind !== "toggle") ||
      !Array.isArray(row.options) ||
      row.options.length === 0
    )
      return null;
    const id = row.id.trim();
    if (seen.has(id)) return null;
    seen.add(id);
    const options: ModelSetting["options"] = [];
    const choices = new Set<string>();
    for (const rawOption of row.options) {
      const option = asRecord(rawOption);
      if (!option || typeof option.value !== "string" || !option.value.trim())
        return null;
      const optionValue = option.value.trim();
      if (choices.has(optionValue)) return null;
      choices.add(optionValue);
      options.push({
        value: optionValue,
        label:
          typeof option.label === "string"
            ? option.label.trim() || optionValue
            : optionValue,
      });
    }
    if (!choices.has(row.value)) return null;
    if (
      row.kind === "toggle" &&
      (!choices.has("true") || !choices.has("false"))
    )
      return null;
    settings.push({
      id,
      label: row.label.trim(),
      kind: row.kind,
      value: row.value,
      options,
      ...(typeof row.description === "string"
        ? { description: row.description }
        : {}),
    });
  }
  return settings;
}
