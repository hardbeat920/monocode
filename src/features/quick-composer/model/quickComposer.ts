import { parseQuickAttachments } from "./quickAttachments";
import { isHarnessAvailable } from "../../../integrations/harness/core/availability";
import { invoke } from "@tauri-apps/api/core";
import { IS_MAC } from "../../../platform/tauri/platform";
import { loadQuickComposerShortcut } from "../../settings/model/settings";
import { pathKey, projectName, prettyParent } from "../../../shared/lib/paths";
import {
  loadArchivedProjects,
  loadPinnedProjects,
  loadProjectRailOrder,
  loadRecents,
  looksLikeProject,
  normalizeProjectPath,
} from "../../projects/model/recents";
import {
  defaultSessionChoice,
  hasLiveCatalog,
  modelsFor,
  resolveModel,
  setHarnessModels,
  type AgentModel,
} from "../../sessions/model/models";
import {
  HARNESSES,
  HARNESS_TITLE,
  RUNTIME_MODES,
  type RuntimeMode,
  type WorkspaceMode,
  type Attachment,
  harnessSupportsAttachments,
  type HarnessId,
} from "../../sessions/model/session";

/** Workspace windows hear this when the panel has a session for them. */
export const QUICK_COMPOSER_LAUNCH_EVENT = "quick_composer_launch";
/** The panel hears this every time the shortcut brings it up. */
export const QUICK_COMPOSER_SHOWN_EVENT = "quick_composer_shown";
/** The panel asks workspace windows for the model lists their CLIs reported. */
export const QUICK_COMPOSER_CATALOG_REQUEST_EVENT =
  "quick_composer_catalog_request";
/** Workspace windows answer with every live catalog they hold. */
export const QUICK_COMPOSER_CATALOG_EVENT = "quick_composer_catalog";

const LAST_PROJECT_KEY = "monocode.quickComposerProject";

/** Live model lists keyed by harness, as a workspace window knows them. */
export type QuickCatalog = {
  models: Partial<Record<HarnessId, AgentModel[]>>;
  availableHarnesses: HarnessId[];
};

export type QuickChoice = { harness: HarnessId; model: string };

export type QuickLaunch = {
  prompt: string;
  /** Create an unsent user draft instead of starting an agent turn. */
  draft?: boolean;
  cwd: string;
  harness: HarnessId;
  /** Missing means the harness default, resolved by the workspace. */
  model?: string;
  modelSettings?: Record<string, string>;
  runtimeMode?: RuntimeMode;
  attachments?: Attachment[];
  workspaceMode?: WorkspaceMode;
  worktreeBase?: string;
  worktreeCwd?: string;
  /** Bring the new session forward instead of starting it quietly. */
  reveal: boolean;
};

/** The panel exists on macOS only; elsewhere the shortcut is never claimed. */
export function quickComposerSupported(): boolean {
  return IS_MAC;
}

export async function setQuickComposerShortcut(
  enabled: boolean,
  shortcut = loadQuickComposerShortcut(),
) {
  if (!quickComposerSupported()) return;
  await invoke("quick_composer_set_enabled", { enabled, shortcut });
}

export function isHarnessId(value: unknown): value is HarnessId {
  return typeof value === "string" && HARNESSES.includes(value as HarnessId);
}

/** The launch as a workspace window receives it. Anything malformed is dropped. */
export function parseQuickLaunch(value: unknown): QuickLaunch | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const attachments = parseQuickAttachments(raw.attachments);
  if (!attachments) return null;
  if (
    typeof raw.prompt !== "string" ||
    (!raw.prompt.trim() && !attachments.length)
  )
    return null;
  if (typeof raw.cwd !== "string" || !raw.cwd.trim()) return null;
  if (!isHarnessId(raw.harness)) return null;
  if (attachments.length && !harnessSupportsAttachments(raw.harness))
    return null;
  if (
    raw.workspaceMode !== undefined &&
    raw.workspaceMode !== "current" &&
    raw.workspaceMode !== "worktree"
  )
    return null;
  if (
    raw.worktreeBase !== undefined &&
    (typeof raw.worktreeBase !== "string" || !raw.worktreeBase.trim())
  )
    return null;
  if (
    raw.worktreeCwd !== undefined &&
    (typeof raw.worktreeCwd !== "string" || !raw.worktreeCwd.trim())
  )
    return null;
  if (raw.workspaceMode === "worktree" && raw.worktreeCwd !== undefined)
    return null;
  if (raw.worktreeBase !== undefined && raw.workspaceMode !== "worktree")
    return null;
  return {
    ...(raw.workspaceMode
      ? { workspaceMode: raw.workspaceMode as WorkspaceMode }
      : {}),
    ...(typeof raw.worktreeBase === "string"
      ? { worktreeBase: raw.worktreeBase }
      : {}),
    ...(typeof raw.worktreeCwd === "string"
      ? { worktreeCwd: raw.worktreeCwd }
      : {}),
    prompt: raw.prompt,
    ...(attachments.length ? { attachments } : {}),
    cwd: raw.cwd,
    harness: raw.harness,
    ...(typeof raw.model === "string" && raw.model ? { model: raw.model } : {}),
    ...(raw.modelSettings &&
    typeof raw.modelSettings === "object" &&
    !Array.isArray(raw.modelSettings)
      ? {
          modelSettings: Object.fromEntries(
            Object.entries(raw.modelSettings).filter(
              (entry): entry is [string, string] =>
                typeof entry[1] === "string",
            ),
          ),
        }
      : {}),
    ...(RUNTIME_MODES.includes(raw.runtimeMode as RuntimeMode)
      ? { runtimeMode: raw.runtimeMode as RuntimeMode }
      : {}),
    reveal: raw.reveal === true,
  };
}

/**
 * The panel is its own webview, so the catalogs a workspace window loaded
 * from each CLI are not in its memory. Only live ones travel: the built-in
 * fallback lists are already bundled on both sides.
 */
export function liveQuickCatalog(): QuickCatalog {
  const catalog: QuickCatalog["models"] = {};
  for (const harness of HARNESSES) {
    if (hasLiveCatalog(harness)) catalog[harness] = modelsFor(harness);
  }
  return {
    models: catalog,
    availableHarnesses: HARNESSES.filter(isHarnessAvailable),
  };
}

export function applyQuickCatalog(value: unknown): HarnessId[] | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (
    !raw.models ||
    typeof raw.models !== "object" ||
    !Array.isArray(raw.availableHarnesses)
  )
    return null;
  for (const [harness, models] of Object.entries(raw.models)) {
    if (!isHarnessId(harness) || !Array.isArray(models)) continue;
    const valid = models.filter(
      (model): model is AgentModel =>
        !!model &&
        typeof model === "object" &&
        typeof (model as AgentModel).id === "string" &&
        typeof (model as AgentModel).name === "string" &&
        (model as AgentModel).harness === harness,
    );
    setHarnessModels(harness, valid);
  }
  return raw.availableHarnesses.filter(isHarnessId);
}

/**
 * Projects to offer, most recently opened first, then whatever else the rail
 * remembers. Archived projects stay out, the same as on the rail.
 */
export function orderQuickProjects(
  recents: string[],
  pinned: string[],
  railOrder: string[],
  archived: string[],
): string[] {
  const hidden = new Set(archived.map(pathKey));
  const seen = new Set<string>();
  const out: string[] = [];
  for (const path of [...recents, ...pinned, ...railOrder]) {
    if (!looksLikeProject(path)) continue;
    const key = pathKey(path);
    if (hidden.has(key) || seen.has(key)) continue;
    seen.add(key);
    out.push(normalizeProjectPath(path));
  }
  return out;
}

export function loadQuickProjects(): string[] {
  return orderQuickProjects(
    loadRecents().map((item) => item.path),
    loadPinnedProjects(),
    loadProjectRailOrder(),
    loadArchivedProjects().map((item) => item.path),
  );
}

export function filterQuickProjects(projects: string[], query: string) {
  const needle = query.trim().toLowerCase();
  if (!needle) return projects;
  return projects.filter(
    (path) =>
      projectName(path).toLowerCase().includes(needle) ||
      prettyParent(path).toLowerCase().includes(needle),
  );
}

/** Every model the picker offers, grouped by harness in picker order. */
export function quickModelOptions(harnesses: HarnessId[]): AgentModel[] {
  return harnesses.flatMap((harness) => modelsFor(harness));
}

/** Matches the model, its upstream provider, or the harness it runs in. */
export function filterQuickModels(models: AgentModel[], query: string) {
  const needle = query.trim().toLowerCase();
  if (!needle) return models;
  return models.filter((model) =>
    [
      model.name,
      model.id,
      model.provider?.name ?? "",
      HARNESS_TITLE[model.harness],
    ]
      .join(" ")
      .toLowerCase()
      .includes(needle),
  );
}

/** The project the last quick session used, if it is still offered. */
export function initialQuickProject(projects: string[]): string | null {
  const last = readString(LAST_PROJECT_KEY);
  if (last) {
    const match = projects.find((path) => pathKey(path) === pathKey(last));
    if (match) return match;
  }
  return projects[0] ?? null;
}

/** Use the Providers default, not a separate last-used quick-composer model. */
export function initialQuickChoice(): QuickChoice {
  return defaultSessionChoice();
}

/** Keep the requested provider/model intact while its live catalog loads. */
export function resolveQuickModel(choice: QuickChoice): AgentModel | null {
  if (
    !hasLiveCatalog(choice.harness) &&
    !modelsFor(choice.harness).some(
      (model) => model.id === choice.model || model.nativeId === choice.model,
    )
  )
    return null;
  const resolved = resolveModel(choice.harness, choice.model);
  return resolved.harness === choice.harness ? resolved : null;
}

export function rememberQuickProject(cwd: string) {
  try {
    localStorage.setItem(LAST_PROJECT_KEY, cwd);
  } catch {
    // private mode / quota
  }
}

function readString(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
