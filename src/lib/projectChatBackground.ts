import {
  CHAT_BACKGROUND_OPACITY_MAX,
  CHAT_BACKGROUND_OPACITY_MIN,
  CHAT_BACKGROUND_SCOPE_DEFAULT,
  loadChatBackgroundEmptyOpacity,
  loadChatBackgroundSessionOpacity,
  loadChatBackgroundScope,
  type ChatBackgroundScope,
} from "./appearance";

const KEY = "monocode:project-chat-backgrounds";

export const PROJECT_CHAT_BACKGROUND_CHANGED =
  "monocode:project-chat-background-changed";

export type ProjectChatBackground = {
  path: string;
  opacity: number;
  scope: ChatBackgroundScope;
};

export type ProjectChatBackgroundSettings = {
  path: string;
  emptyOpacity: number;
  sessionOpacity: number;
  scope: ChatBackgroundScope;
};

type StoredProjectChatBackground = Partial<ProjectChatBackground> & {
  emptyOpacity?: number;
  sessionOpacity?: number;
};

let revision = Date.now();

function clampOpacity(value: number): number {
  return Math.min(
    CHAT_BACKGROUND_OPACITY_MAX,
    Math.max(CHAT_BACKGROUND_OPACITY_MIN, value),
  );
}

function read(): Record<string, StoredProjectChatBackground> {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    return parsed && typeof parsed === "object"
      ? (parsed as Record<string, StoredProjectChatBackground>)
      : {};
  } catch {
    return {};
  }
}

function write(value: Record<string, StoredProjectChatBackground>) {
  try {
    localStorage.setItem(KEY, JSON.stringify(value));
  } catch {
    // private mode / quota
  }
}

function validScope(value: unknown): value is ChatBackgroundScope {
  return value === "empty" || value === "all";
}

function storedOpacity(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? clampOpacity(value)
    : fallback;
}

export function loadProjectChatBackgroundSettings(
  project: string,
): ProjectChatBackgroundSettings | null {
  const stored = read()[project];
  const path = typeof stored?.path === "string" ? stored.path.trim() : "";
  if (!path) return null;
  const legacyOpacity = storedOpacity(
    stored.opacity,
    loadChatBackgroundEmptyOpacity(),
  );
  const hasStoredOpacity =
    typeof stored.opacity === "number" && Number.isFinite(stored.opacity);
  const hasStoredEmptyOpacity =
    typeof stored.emptyOpacity === "number" &&
    Number.isFinite(stored.emptyOpacity);
  return {
    path,
    emptyOpacity: storedOpacity(stored.emptyOpacity, legacyOpacity),
    sessionOpacity: storedOpacity(
      stored.sessionOpacity,
      hasStoredEmptyOpacity
        ? storedOpacity(stored.emptyOpacity, legacyOpacity)
        : hasStoredOpacity
          ? legacyOpacity
          : loadChatBackgroundSessionOpacity(),
    ),
    scope: validScope(stored.scope) ? stored.scope : loadChatBackgroundScope(),
  };
}

export function saveProjectChatBackgroundSettings(
  project: string,
  value: ProjectChatBackgroundSettings,
) {
  const path = value.path.trim();
  if (!project || !path) return;
  const next = read();
  next[project] = {
    path,
    emptyOpacity: clampOpacity(value.emptyOpacity),
    sessionOpacity: clampOpacity(value.sessionOpacity),
    scope: validScope(value.scope)
      ? value.scope
      : CHAT_BACKGROUND_SCOPE_DEFAULT,
  };
  write(next);
  notifyProjectChatBackgroundChanged();
}

export function loadProjectChatBackground(
  project: string,
): ProjectChatBackground | null {
  const settings = loadProjectChatBackgroundSettings(project);
  if (!settings) return null;
  return {
    path: settings.path,
    opacity: settings.emptyOpacity,
    scope: settings.scope,
  };
}

export function saveProjectChatBackground(
  project: string,
  value: ProjectChatBackground,
) {
  const path = value.path.trim();
  if (!project || !path) return;
  const next = read();
  next[project] = {
    path,
    opacity: clampOpacity(value.opacity),
    scope: validScope(value.scope)
      ? value.scope
      : CHAT_BACKGROUND_SCOPE_DEFAULT,
  };
  write(next);
  notifyProjectChatBackgroundChanged();
}

export function clearProjectChatBackgroundSetting(project: string) {
  const next = read();
  if (!(project in next)) return;
  delete next[project];
  write(next);
  notifyProjectChatBackgroundChanged();
}

export function notifyProjectChatBackgroundChanged() {
  revision += 1;
  window.dispatchEvent(new CustomEvent(PROJECT_CHAT_BACKGROUND_CHANGED));
}

export function projectChatBackgroundRevision(): number {
  return revision;
}

export function subscribeProjectChatBackground(listener: () => void) {
  window.addEventListener(PROJECT_CHAT_BACKGROUND_CHANGED, listener);
  return () =>
    window.removeEventListener(PROJECT_CHAT_BACKGROUND_CHANGED, listener);
}
