import {
  resolveProjectLocation,
  type ProjectLocation,
} from "../../../platform/tauri/fs";
import { pathKey } from "../../../shared/lib/paths";
import {
  looksLikeProject,
  normalizeProjectPath,
  sameProjectPath,
} from "./recents";

const KEY = "monocode.projectLocations";

type StoredProjectLocation = {
  path: string;
  identity: string;
};

export type ProjectLocationSync = ProjectLocation & {
  moved: boolean;
};

function read(): Record<string, StoredProjectLocation> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const locations: Record<string, StoredProjectLocation> = {};
    for (const [key, value] of Object.entries(parsed)) {
      if (!value || typeof value !== "object") continue;
      const candidate = value as Partial<StoredProjectLocation>;
      if (
        typeof candidate.path !== "string" ||
        !candidate.path ||
        typeof candidate.identity !== "string" ||
        !candidate.identity
      ) {
        continue;
      }
      locations[key] = {
        path: normalizeProjectPath(candidate.path),
        identity: candidate.identity,
      };
    }
    return locations;
  } catch {
    return {};
  }
}

function write(locations: Record<string, StoredProjectLocation>) {
  try {
    localStorage.setItem(KEY, JSON.stringify(locations));
  } catch {
    // private mode / quota
  }
}

function remember(
  locations: Record<string, StoredProjectLocation>,
  location: ProjectLocation,
) {
  const path = normalizeProjectPath(location.path);
  locations[pathKey(path)] = { path, identity: location.identity };
  write(locations);
}

/** Capture an identity without searching for a moved folder. */
export async function rememberProjectLocation(path: string): Promise<void> {
  const normalized = normalizeProjectPath(path);
  if (!looksLikeProject(normalized)) return;
  const location = await resolveProjectLocation(normalized);
  if (!location) return;
  remember(read(), location);
}

/** Resolve a missing project among its former siblings and persist the result. */
export async function synchronizeProjectLocation(
  path: string,
): Promise<ProjectLocationSync | null> {
  const normalized = normalizeProjectPath(path);
  if (!looksLikeProject(normalized)) {
    return { path: normalized, identity: "", moved: false };
  }
  const locations = read();
  const oldKey = pathKey(normalized);
  const location = await resolveProjectLocation(
    normalized,
    locations[oldKey]?.identity,
  );
  if (!location) return null;

  const resolved = normalizeProjectPath(location.path);
  const moved = !sameProjectPath(normalized, resolved);
  if (moved) delete locations[oldKey];
  remember(locations, { ...location, path: resolved });
  return { ...location, path: resolved, moved };
}

export function forgetProjectLocation(path: string): void {
  const locations = read();
  const key = pathKey(normalizeProjectPath(path));
  if (!(key in locations)) return;
  delete locations[key];
  write(locations);
}
