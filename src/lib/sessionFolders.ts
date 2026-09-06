import { isHexColor } from "./colorUtils";
import { compareSessionSummaries } from "./sessionHistory";
import type { SessionSummary } from "./sessionStore";
import { normalizeProjectPath } from "./recents";
import { orderByIds } from "./reorder";
import { TAB_GROUP_COLORS } from "./tabGroups";

const KEY = "monocode.sessionFolders";

export type SessionFolder = {
  id: string;
  name: string;
  sessionIds: string[];
  collapsed: boolean;
  /** Palette index from `TAB_GROUP_COLORS`. Missing or 0 is the default wash. */
  colorIndex?: number;
  /** Custom hex from the folder color picker. Wins over `colorIndex`. */
  customColor?: string;
};

export type SessionListDropTarget =
  | { kind: "folder"; id: string }
  | { kind: "session"; id: string };

export type SessionListEntry =
  | {
      kind: "folder";
      folder: SessionFolder;
      sessions: SessionSummary[];
    }
  | { kind: "session"; session: SessionSummary }
  | { kind: "divider" };

type StoredFolder = {
  id?: unknown;
  name?: unknown;
  sessionIds?: unknown;
  collapsed?: unknown;
  colorIndex?: unknown;
  customColor?: unknown;
};

export function folderContaining(
  folders: SessionFolder[],
  sessionId: string,
): SessionFolder | undefined {
  return folders.find((folder) => folder.sessionIds.includes(sessionId));
}

export function uniqueFolderName(
  folders: SessionFolder[],
  base = "New folder",
): string {
  const names = new Set(folders.map((folder) => folder.name));
  if (!names.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base} ${n}`;
    if (!names.has(candidate)) return candidate;
  }
}

export function ungroupedSessions(
  sessions: SessionSummary[],
  folders: SessionFolder[],
): SessionSummary[] {
  const grouped = new Set<string>();
  for (const folder of folders) {
    for (const id of folder.sessionIds) grouped.add(id);
  }
  return sessions.filter((session) => !grouped.has(session.id));
}

/** Open tabs that belong to a folder but are not in history yet (blank chats). */
export function mergeFolderSessionSummaries(
  visible: SessionSummary[],
  extras: readonly SessionSummary[],
  folders: SessionFolder[],
): SessionSummary[] {
  if (extras.length === 0 || folders.length === 0) return visible;
  const grouped = new Set<string>();
  for (const folder of folders) {
    for (const id of folder.sessionIds) grouped.add(id);
  }
  const have = new Set(visible.map((session) => session.id));
  const extra = extras.filter(
    (session) => grouped.has(session.id) && !have.has(session.id),
  );
  if (extra.length === 0) return visible;
  return [...visible, ...extra];
}

/**
 * Folders first (stored order), then ungrouped sessions. A divider still
 * splits pinned and unpinned ungrouped cards — the same split the flat
 * list already used.
 */
export function buildSessionList(
  visible: SessionSummary[],
  folders: SessionFolder[],
  ungrouped: SessionSummary[],
): SessionListEntry[] {
  const byId = new Map(visible.map((session) => [session.id, session]));
  const entries: SessionListEntry[] = [];
  for (const folder of folders) {
    const members: SessionSummary[] = [];
    for (const id of folder.sessionIds) {
      const session = byId.get(id);
      if (session) members.push(session);
    }
    if (members.length === 0) continue;
    members.sort(compareSessionSummaries);
    entries.push({ kind: "folder", folder, sessions: members });
  }
  for (let index = 0; index < ungrouped.length; index += 1) {
    const session = ungrouped[index];
    const prev = ungrouped[index - 1];
    if (prev?.pinned && !session.pinned) entries.push({ kind: "divider" });
    entries.push({ kind: "session", session });
  }
  return entries;
}

export function sessionListNavigationIds(
  entries: readonly SessionListEntry[],
  expandCollapsed: boolean,
): string[] {
  const ids: string[] = [];
  for (const entry of entries) {
    if (entry.kind === "session") {
      ids.push(entry.session.id);
      continue;
    }
    if (entry.kind !== "folder") continue;
    if (entry.folder.collapsed && !expandCollapsed) continue;
    ids.push(...entry.sessions.map((session) => session.id));
  }
  return ids;
}

export function createFolderWithSessions(
  folders: SessionFolder[],
  sessionIds: string[],
  name?: string,
): { folders: SessionFolder[]; id: string } {
  const ids = uniqueIds(sessionIds);
  if (ids.length === 0) return { folders, id: "" };
  const next = removeSessions(folders, ids);
  const id = crypto.randomUUID();
  const folder: SessionFolder = {
    id,
    name: name ?? uniqueFolderName(next),
    sessionIds: ids,
    collapsed: false,
  };
  return { folders: [folder, ...next], id };
}

export function addSessionToFolder(
  folders: SessionFolder[],
  folderId: string,
  sessionId: string,
): SessionFolder[] {
  if (!sessionId || !folders.some((folder) => folder.id === folderId)) {
    return folders;
  }
  const current = folderContaining(folders, sessionId);
  if (current?.id === folderId) return folders;
  return removeEmpty(
    folders.map((folder) => {
      if (folder.id === folderId) {
        return { ...folder, sessionIds: [...folder.sessionIds, sessionId] };
      }
      if (!folder.sessionIds.includes(sessionId)) return folder;
      return {
        ...folder,
        sessionIds: folder.sessionIds.filter((id) => id !== sessionId),
      };
    }),
  );
}

export function removeSessionFromFolder(
  folders: SessionFolder[],
  sessionId: string,
): SessionFolder[] {
  if (!folderContaining(folders, sessionId)) return folders;
  return removeEmpty(
    folders.map((folder) =>
      folder.sessionIds.includes(sessionId)
        ? {
            ...folder,
            sessionIds: folder.sessionIds.filter((id) => id !== sessionId),
          }
        : folder,
    ),
  );
}

export function dissolveFolder(
  folders: SessionFolder[],
  folderId: string,
): SessionFolder[] {
  if (!folders.some((folder) => folder.id === folderId)) return folders;
  return folders.filter((folder) => folder.id !== folderId);
}

export function renameFolder(
  folders: SessionFolder[],
  folderId: string,
  name: string,
): SessionFolder[] {
  const trimmed = name.trim();
  if (!trimmed) return folders;
  const folder = folders.find((entry) => entry.id === folderId);
  if (!folder || folder.name === trimmed) return folders;
  return folders.map((entry) =>
    entry.id === folderId ? { ...entry, name: trimmed } : entry,
  );
}

export function setFolderCollapsed(
  folders: SessionFolder[],
  folderId: string,
  collapsed: boolean,
): SessionFolder[] {
  const folder = folders.find((entry) => entry.id === folderId);
  if (!folder || folder.collapsed === collapsed) return folders;
  return folders.map((entry) =>
    entry.id === folderId ? { ...entry, collapsed } : entry,
  );
}

export function setFolderColor(
  folders: SessionFolder[],
  folderId: string,
  colorIndex: number | null,
): SessionFolder[] {
  const folder = folders.find((entry) => entry.id === folderId);
  const nextIndex = sanitizeColorIndex(colorIndex);
  if (
    !folder ||
    (folder.colorIndex === nextIndex && folder.customColor == null)
  ) {
    return folders;
  }
  return folders.map((entry) => {
    if (entry.id !== folderId) return entry;
    const next = withoutColors(entry);
    return nextIndex == null ? next : { ...next, colorIndex: nextIndex };
  });
}

export function setFolderCustomColor(
  folders: SessionFolder[],
  folderId: string,
  color: string | null,
): SessionFolder[] {
  const folder = folders.find((entry) => entry.id === folderId);
  if (!folder) return folders;
  if (color == null) {
    if (folder.customColor == null) return folders;
    return folders.map((entry) =>
      entry.id === folderId ? withoutColors(entry) : entry,
    );
  }
  const hex = parseCustomHex(color);
  if (hex == null) return folders;
  if (folder.customColor === hex && folder.colorIndex == null) return folders;
  return folders.map((entry) =>
    entry.id === folderId
      ? { ...withoutColors(entry), customColor: hex }
      : entry,
  );
}

/** Reorder only the named folders; anything else keeps its slot. */
export function reorderSessionFolders(
  folders: SessionFolder[],
  ids: string[],
): SessionFolder[] {
  const idSet = new Set(ids);
  const moving = folders.filter((folder) => idSet.has(folder.id));
  const ordered = orderByIds(moving, ids);
  if (
    ordered.length !== moving.length ||
    ordered.every((folder, index) => folder.id === moving[index]?.id)
  ) {
    return folders;
  }
  let next = 0;
  return folders.map((folder) =>
    idSet.has(folder.id) ? ordered[next++]! : folder,
  );
}

/** Saturated project palette or custom hex, or undefined for the default wash. */
export function folderAccent(
  colorIndex: number | undefined,
  customColor?: string,
): string | undefined {
  const hex = parseCustomHex(customColor);
  if (hex) return hex;
  const index = sanitizeColorIndex(colorIndex ?? null);
  return index == null ? undefined : TAB_GROUP_COLORS[index];
}

/** Quiet fill so a folder tint never reads as a solid chip. */
export function folderShellFill(
  colorIndex: number | undefined,
  customColor?: string,
): string | undefined {
  const accent = folderAccent(colorIndex, customColor);
  if (!accent) return undefined;
  return `color-mix(in srgb, ${accent} 18%, transparent)`;
}

function sanitizeColorIndex(colorIndex: number | null): number | undefined {
  if (
    colorIndex == null ||
    !Number.isInteger(colorIndex) ||
    colorIndex <= 0 ||
    colorIndex >= TAB_GROUP_COLORS.length
  ) {
    return undefined;
  }
  return colorIndex;
}

/**
 * Dropping on a folder (or a session already in one) joins that folder.
 * Dropping on an ungrouped session opens a new folder around both.
 */
export function applySessionListDrop(
  folders: SessionFolder[],
  draggedId: string,
  target: SessionListDropTarget,
): { folders: SessionFolder[]; createdId?: string } {
  if (!draggedId) return { folders };
  if (target.kind === "session" && target.id === draggedId) {
    return { folders };
  }
  if (target.kind === "folder") {
    const next = addSessionToFolder(folders, target.id, draggedId);
    return { folders: setFolderCollapsed(next, target.id, false) };
  }
  const dest = folderContaining(folders, target.id);
  if (dest) {
    if (folderContaining(folders, draggedId)?.id === dest.id) {
      return { folders };
    }
    const next = addSessionToFolder(folders, dest.id, draggedId);
    return { folders: setFolderCollapsed(next, dest.id, false) };
  }
  const { folders: next, id } = createFolderWithSessions(folders, [
    draggedId,
    target.id,
  ]);
  return id ? { folders: next, createdId: id } : { folders };
}

export function pruneSessionFolders(
  folders: SessionFolder[],
  knownIds: ReadonlySet<string>,
): SessionFolder[] {
  let changed = false;
  const next: SessionFolder[] = [];
  for (const folder of folders) {
    const sessionIds = folder.sessionIds.filter((id) => knownIds.has(id));
    if (sessionIds.length === 0) {
      changed = true;
      continue;
    }
    if (sessionIds.length !== folder.sessionIds.length) {
      changed = true;
      next.push({ ...folder, sessionIds });
    } else {
      next.push(folder);
    }
  }
  return changed ? next : folders;
}

export function loadSessionFolders(cwd: string): SessionFolder[] {
  const key = storageKey(cwd);
  if (!key) return [];
  return parseStore()[key] ?? [];
}

export function saveSessionFolders(cwd: string, folders: SessionFolder[]): void {
  const key = storageKey(cwd);
  if (!key) return;
  try {
    const store = parseStore();
    if (folders.length === 0) delete store[key];
    else store[key] = folders;
    localStorage.setItem(KEY, JSON.stringify(store));
  } catch {
    // private mode / quota
  }
}

function storageKey(cwd: string): string | null {
  if (!cwd || cwd === "~") return null;
  return normalizeProjectPath(cwd);
}

function parseStore(): Record<string, SessionFolder[]> {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const out: Record<string, SessionFolder[]> = {};
    for (const [cwd, value] of Object.entries(parsed)) {
      if (!cwd || cwd === "~") continue;
      const folders = parseFolders(value);
      if (folders.length > 0) out[normalizeProjectPath(cwd)] = folders;
    }
    return out;
  } catch {
    return {};
  }
}

function parseFolders(value: unknown): SessionFolder[] {
  if (!Array.isArray(value)) return [];
  const out: SessionFolder[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    const folder = parseFolder(item);
    if (!folder || seen.has(folder.id)) continue;
    seen.add(folder.id);
    out.push(folder);
  }
  return out;
}

function parseFolder(value: unknown): SessionFolder | null {
  if (!value || typeof value !== "object") return null;
  const rec = value as StoredFolder;
  if (typeof rec.id !== "string" || !rec.id) return null;
  if (typeof rec.name !== "string") return null;
  const name = rec.name.trim();
  if (!name) return null;
  if (!Array.isArray(rec.sessionIds)) return null;
  const sessionIds = uniqueIds(
    rec.sessionIds.filter((id): id is string => typeof id === "string" && !!id),
  );
  if (sessionIds.length === 0) return null;
  const customColor = parseCustomHex(
    typeof rec.customColor === "string" ? rec.customColor : null,
  );
  const colorIndex = sanitizeColorIndex(
    typeof rec.colorIndex === "number" ? rec.colorIndex : null,
  );
  return {
    id: rec.id,
    name,
    sessionIds,
    collapsed: rec.collapsed === true,
    ...(customColor != null
      ? { customColor }
      : colorIndex != null
        ? { colorIndex }
        : {}),
  };
}

function parseCustomHex(color: string | null | undefined): string | undefined {
  if (typeof color !== "string" || !isHexColor(color)) return undefined;
  return color.toLowerCase();
}

function withoutColors(folder: SessionFolder): SessionFolder {
  const { colorIndex: _index, customColor: _custom, ...rest } = folder;
  return rest;
}

function uniqueIds(ids: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const id of ids) {
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

function removeSessions(
  folders: SessionFolder[],
  sessionIds: string[],
): SessionFolder[] {
  const drop = new Set(sessionIds);
  return removeEmpty(
    folders.map((folder) => {
      if (!folder.sessionIds.some((id) => drop.has(id))) return folder;
      return {
        ...folder,
        sessionIds: folder.sessionIds.filter((id) => !drop.has(id)),
      };
    }),
  );
}

function removeEmpty(folders: SessionFolder[]): SessionFolder[] {
  return folders.filter((folder) => folder.sessionIds.length > 0);
}
