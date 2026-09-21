import { pathKey } from "../../../shared/lib/paths";
import { PROJECT_MASCOTS } from "./projectMascots";
import { TAB_GROUP_COLORS } from "../../workspace/model/tabGroups";

const GROUPS_KEY = "monocode.projectGroups";
const ASSIGNMENTS_KEY = "monocode.projectGroupAssignments";
const HEX_COLOR_RE = /^#[0-9a-fA-F]{6}$/;

export type ProjectGroup = {
  id: string;
  name: string;
  collapsed: boolean;
  colorIndex?: number;
  customColor?: string;
  mascot?: string;
};

function normalizeGroup(value: unknown): ProjectGroup | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<ProjectGroup>;
  if (typeof candidate.id !== "string" || !candidate.id.trim()) return null;
  if (typeof candidate.name !== "string" || !candidate.name.trim()) return null;

  const colorIndex =
    typeof candidate.colorIndex === "number" &&
    Number.isInteger(candidate.colorIndex) &&
    candidate.colorIndex >= 0 &&
    candidate.colorIndex < TAB_GROUP_COLORS.length
      ? candidate.colorIndex
      : undefined;
  const customColor =
    typeof candidate.customColor === "string" &&
    HEX_COLOR_RE.test(candidate.customColor)
      ? candidate.customColor.toLowerCase()
      : undefined;
  const mascot =
    typeof candidate.mascot === "string" &&
    PROJECT_MASCOTS.some((item) => item.name === candidate.mascot)
      ? candidate.mascot
      : undefined;

  return {
    id: candidate.id,
    name: candidate.name.trim(),
    collapsed: candidate.collapsed === true,
    ...(customColor
      ? { customColor }
      : colorIndex == null
        ? {}
        : { colorIndex }),
    ...(mascot ? { mascot } : {}),
  };
}

export function loadProjectGroups(): ProjectGroup[] {
  try {
    const parsed: unknown = JSON.parse(
      localStorage.getItem(GROUPS_KEY) ?? "[]",
    );
    if (!Array.isArray(parsed)) return [];
    const groups: ProjectGroup[] = [];
    const seen = new Set<string>();
    for (const value of parsed) {
      const group = normalizeGroup(value);
      if (!group || seen.has(group.id)) continue;
      seen.add(group.id);
      groups.push(group);
    }
    return groups;
  } catch {
    return [];
  }
}

export function saveProjectGroups(groups: ProjectGroup[]): boolean {
  const normalized = groups.flatMap((group) => {
    const value = normalizeGroup(group);
    return value ? [value] : [];
  });
  try {
    localStorage.setItem(GROUPS_KEY, JSON.stringify(normalized));
    return true;
  } catch {
    return false;
  }
}

export function loadProjectGroupAssignments(
  groups = loadProjectGroups(),
): Record<string, string> {
  try {
    const parsed: unknown = JSON.parse(
      localStorage.getItem(ASSIGNMENTS_KEY) ?? "{}",
    );
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
      return {};
    const groupIds = new Set(groups.map((group) => group.id));
    return Object.fromEntries(
      Object.entries(parsed).filter(
        (entry): entry is [string, string] =>
          Boolean(entry[0]) &&
          typeof entry[1] === "string" &&
          groupIds.has(entry[1]),
      ),
    );
  } catch {
    return {};
  }
}

export function saveProjectGroupAssignments(
  assignments: Record<string, string>,
): boolean {
  try {
    localStorage.setItem(ASSIGNMENTS_KEY, JSON.stringify(assignments));
    return true;
  } catch {
    return false;
  }
}

export function projectGroupIdForPath(
  path: string,
  assignments: Record<string, string>,
): string | undefined {
  return assignments[pathKey(path)];
}

export function setProjectGroupAssignment(
  path: string,
  groupId: string | null,
): Record<string, string> {
  const next = loadProjectGroupAssignments();
  const key = pathKey(path);
  if (groupId == null) delete next[key];
  else if (loadProjectGroups().some((group) => group.id === groupId)) {
    next[key] = groupId;
  }
  saveProjectGroupAssignments(next);
  return next;
}

export function removeProjectGroupAssignment(path: string): void {
  setProjectGroupAssignment(path, null);
}

export function nextProjectGroupName(groups: ProjectGroup[]): string {
  const names = new Set(groups.map((group) => group.name.toLocaleLowerCase()));
  if (!names.has("new group")) return "New group";
  for (let suffix = 2; ; suffix += 1) {
    const name = `New group ${suffix}`;
    if (!names.has(name.toLocaleLowerCase())) return name;
  }
}

export function createProjectGroup(groups: ProjectGroup[]): ProjectGroup {
  const id =
    globalThis.crypto?.randomUUID?.() ??
    `group-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 9)}`;
  return {
    id,
    name: nextProjectGroupName(groups),
    collapsed: false,
  };
}
