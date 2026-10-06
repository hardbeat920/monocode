import { pathKey } from "../../../shared/lib/paths";
import {
  defaultSessionChoice,
  firstEnabledHarness,
  modelsFor,
  preferredModelId,
} from "../../sessions/model/models";
import { HARNESSES, type HarnessId } from "../../sessions/model/session";

const KEY = "monocode.figmaGenerationModel.v1";

export const FIGMA_MODELS_CHANGE_EVENT = "monocode:figma-models-change";

export type FigmaModelChoice = {
  harness: HarnessId;
  model: string;
  modelSettings: Record<string, string>;
};

export type FigmaModelSource = "picked" | "figma" | "project";

export type ResolvedFigmaModel = {
  choice: FigmaModelChoice;
  source: FigmaModelSource;
};

let revision = 0;
let cache: FigmaModelChoice | null = null;
let cacheRaw: string | null = null;
const picks = new Map<string, FigmaModelChoice>();

export function figmaModelsRevision(): number {
  return revision;
}

export function loadFigmaDefaultModel(): FigmaModelChoice | null {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(KEY);
  } catch {
    raw = null;
  }
  if (raw === cacheRaw) return cache;
  cacheRaw = raw;
  cache = parseChoice(raw);
  return cache;
}

export function saveFigmaDefaultModel(choice: FigmaModelChoice | null): void {
  const next = choice ? normalizeChoice(choice) : null;
  const serialized = next ? JSON.stringify(next) : null;
  if (serialized) localStorage.setItem(KEY, serialized);
  else localStorage.removeItem(KEY);
  cache = next;
  cacheRaw = serialized;
  notify();
}

export function pickFigmaModel(cwd: string, choice: FigmaModelChoice): void {
  picks.set(pathKey(cwd), normalizeChoice(choice));
  notify();
}

export function clearFigmaModelPick(cwd: string): void {
  if (!picks.delete(pathKey(cwd))) return;
  notify();
}

export function resolveFigmaModel(cwd: string): ResolvedFigmaModel {
  const picked = picks.get(pathKey(cwd)) ?? null;
  const configured = picked ?? loadFigmaDefaultModel();
  const project = defaultSessionChoice(cwd);
  const harness = firstEnabledHarness(
    cwd,
    configured?.harness ?? project.harness,
  );
  const model =
    (configured?.harness === harness ? configured.model : undefined) ??
    (project.harness === harness ? project.model : undefined) ??
    modelsFor(harness)[0]?.id ??
    preferredModelId(harness);
  const modelSettings =
    configured?.harness === harness && configured.model === model
      ? configured.modelSettings
      : {};
  return {
    choice: { harness, model, modelSettings },
    source: picked ? "picked" : configured ? "figma" : "project",
  };
}

export function subscribeFigmaModels(listener: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const storage = (event: StorageEvent) => {
    if (event.key !== KEY) return;
    cache = null;
    cacheRaw = null;
    revision += 1;
    listener();
  };
  window.addEventListener(FIGMA_MODELS_CHANGE_EVENT, listener);
  window.addEventListener("storage", storage);
  return () => {
    window.removeEventListener(FIGMA_MODELS_CHANGE_EVENT, listener);
    window.removeEventListener("storage", storage);
  };
}

function notify(): void {
  revision += 1;
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(FIGMA_MODELS_CHANGE_EVENT));
}

function parseChoice(raw: string | null): FigmaModelChoice | null {
  if (!raw) return null;
  try {
    return choiceFrom(JSON.parse(raw));
  } catch {
    return null;
  }
}

function choiceFrom(value: unknown): FigmaModelChoice | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const harness = HARNESSES.find((id) => id === record.harness);
  if (!harness || typeof record.model !== "string" || !record.model)
    return null;
  return {
    harness,
    model: record.model,
    modelSettings: settingsFrom(record.modelSettings),
  };
}

function settingsFrom(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return Object.fromEntries(
    Object.entries(value).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  );
}

function normalizeChoice(choice: FigmaModelChoice): FigmaModelChoice {
  return {
    harness: choice.harness,
    model: choice.model,
    modelSettings: settingsFrom(choice.modelSettings),
  };
}
