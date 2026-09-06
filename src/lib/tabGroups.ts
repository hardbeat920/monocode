import { t } from "./i18n";
import type { Tab } from "../chrome/TitleBar";

/** Chrome-like palette — saturated enough to read on dark glass. */
export const TAB_GROUP_COLORS = [
  "hsl(210 8% 58%)",
  "hsl(211 92% 62%)",
  "hsl(12 80% 58%)",
  "hsl(45 90% 55%)",
  "hsl(142 55% 50%)",
  "hsl(330 70% 62%)",
  "hsl(280 55% 62%)",
  "hsl(175 55% 48%)",
  "hsl(25 85% 58%)",
] as const;

export function tabGroupColor(project: string): string {
  let hash = 0;
  for (let i = 0; i < project.length; i++) {
    hash = (hash * 31 + project.charCodeAt(i)) >>> 0;
  }
  return TAB_GROUP_COLORS[(hash % (TAB_GROUP_COLORS.length - 1)) + 1];
}

const COLOR_KEY = "monocode:tab-group:colors";
const CUSTOM_COLOR_KEY = "monocode:tab-group:custom-colors";
const LABEL_KEY = "monocode:tab-group:labels";
const LOGO_KEY = "monocode:tab-group:logos";
const MASCOT_KEY = "monocode:tab-group:mascots";

const HEX_COLOR_RE = /^#[0-9a-fA-F]{6}$/;

export const TAB_GROUP_LOGOS_CHANGED = "monocode:tab-group-logos-changed";

let logoDisplayRevision = 0;

export function tabGroupLogoDisplayRevision(): number {
  return logoDisplayRevision;
}

function readRecord(key: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(
        (entry): entry is [string, string] =>
          typeof entry[0] === "string" && typeof entry[1] === "string",
      ),
    );
  } catch {
    return {};
  }
}

function writeRecord(key: string, value: Record<string, string>): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* ignore quota errors */
  }
}

export function loadTabGroupColors(): Record<string, number> {
  const raw = readRecord(COLOR_KEY);
  const out: Record<string, number> = {};
  for (const [project, value] of Object.entries(raw)) {
    const index = Number.parseInt(value, 10);
    if (index >= 0 && index < TAB_GROUP_COLORS.length) out[project] = index;
  }
  return out;
}

export function loadTabGroupCustomColors(): Record<string, string> {
  const raw = readRecord(CUSTOM_COLOR_KEY);
  const out: Record<string, string> = {};
  for (const [project, value] of Object.entries(raw)) {
    if (HEX_COLOR_RE.test(value)) out[project] = value.toLowerCase();
  }
  return out;
}

function writeTabGroupColorIndices(next: Record<string, number>): void {
  writeRecord(
    COLOR_KEY,
    Object.fromEntries(Object.entries(next).map(([k, v]) => [k, String(v)])),
  );
}

function writeTabGroupCustomColors(next: Record<string, string>): void {
  writeRecord(CUSTOM_COLOR_KEY, next);
}

function clearTabGroupColorIndex(project: string): void {
  const next = loadTabGroupColors();
  if (!(project in next)) return;
  delete next[project];
  writeTabGroupColorIndices(next);
}

function clearTabGroupCustomColor(project: string): void {
  const next = loadTabGroupCustomColors();
  if (!(project in next)) return;
  delete next[project];
  writeTabGroupCustomColors(next);
}

export function saveTabGroupColor(project: string, index: number | null): void {
  clearTabGroupCustomColor(project);
  const next = loadTabGroupColors();
  if (index == null) delete next[project];
  else next[project] = index;
  writeTabGroupColorIndices(next);
}

export function saveTabGroupCustomColor(
  project: string,
  color: string | null,
): void {
  clearTabGroupColorIndex(project);
  const next = loadTabGroupCustomColors();
  if (color == null || !HEX_COLOR_RE.test(color)) delete next[project];
  else next[project] = color.toLowerCase();
  writeTabGroupCustomColors(next);
}

export function loadTabGroupLabels(): Record<string, string> {
  return readRecord(LABEL_KEY);
}

export function saveTabGroupLabel(project: string, label: string): void {
  const trimmed = label.trim();
  const next = loadTabGroupLabels();
  if (!trimmed) delete next[project];
  else next[project] = trimmed;
  writeRecord(LABEL_KEY, next);
}

export function loadTabGroupLogos(): Record<string, string> {
  return readRecord(LOGO_KEY);
}

export function saveTabGroupLogo(project: string, path: string | null): void {
  const next = loadTabGroupLogos();
  if (!path) delete next[project];
  else next[project] = path;
  writeRecord(LOGO_KEY, next);
}

export function loadTabGroupMascots(): Record<string, string> {
  return readRecord(MASCOT_KEY);
}

/** `null` restores the mascot picked from the project's name. */
export function saveTabGroupMascot(project: string, name: string | null): void {
  const next = loadTabGroupMascots();
  if (!name) delete next[project];
  else next[project] = name;
  writeRecord(MASCOT_KEY, next);
}

/** Drops every saved appearance override for a project. */
export function clearTabGroupSettings(project: string): void {
  for (const key of [
    COLOR_KEY,
    CUSTOM_COLOR_KEY,
    LABEL_KEY,
    LOGO_KEY,
    MASCOT_KEY,
  ]) {
    const next = readRecord(key);
    if (!(project in next)) continue;
    delete next[project];
    writeRecord(key, next);
  }
}

export function resolveTabGroupMascot(
  project: string,
  overrides?: Record<string, string>,
): string | null {
  return overrides?.[project] ?? null;
}

export function resolveTabGroupLogo(
  project: string,
  overrides?: Record<string, string>,
): string | null {
  return overrides?.[project] ?? null;
}

export function notifyTabGroupLogosChanged(): void {
  logoDisplayRevision += 1;
  window.dispatchEvent(new CustomEvent(TAB_GROUP_LOGOS_CHANGED));
}

export function resolveTabGroupColor(
  project: string,
  overrides?: Record<string, number>,
  customOverrides?: Record<string, string>,
  fallbackKey?: string,
): string {
  const custom = customOverrides?.[project];
  if (custom && HEX_COLOR_RE.test(custom)) return custom;

  const index = overrides?.[project];
  if (index != null && index >= 0 && index < TAB_GROUP_COLORS.length) {
    return TAB_GROUP_COLORS[index];
  }
  return tabGroupColor(fallbackKey || project);
}

export function resolveTabGroupCustomColor(
  project: string,
  overrides?: Record<string, string>,
): string | null {
  const custom = overrides?.[project];
  return custom && HEX_COLOR_RE.test(custom) ? custom : null;
}

export function resolveTabGroupLabel(
  project: string,
  overrides?: Record<string, string>,
  fallback = t("Group"),
): string {
  return overrides?.[project]?.trim() || fallback;
}

export function resolveTabGroupColorIndex(
  project: string,
  overrides?: Record<string, number>,
  customOverrides?: Record<string, string>,
): number | null {
  if (resolveTabGroupCustomColor(project, customOverrides)) return null;

  const index = overrides?.[project];
  if (index != null && index >= 0 && index < TAB_GROUP_COLORS.length) {
    return index;
  }
  return null;
}

export type TabGroupSegment =
  | { kind: "single"; tab: Tab; index: number }
  | {
      kind: "group";
      project: string;
      tabs: Tab[];
      startIndex: number;
      color: string;
      key: string;
    };

export type GroupedTab = { id: string; groupId?: string };

/**
 * Groups never span projects, so every membership change is checked against the
 * project each tab belongs to. Callers supply the lookup because the project is
 * derived from the tab's session, not stored on the tab.
 */
export type TabProjectLookup = (tabId: string) => string | undefined;

/** An unknown project is a wildcard — nothing to scope against. */
function projectKey(project: string | undefined): string {
  return project?.trim() ?? "";
}

function sameProject(a: string, b: string): boolean {
  return !a || !b || a === b;
}

/** The project a group belongs to, or null when it has none in common. */
export function tabGroupProject<T extends GroupedTab>(
  tabs: T[],
  groupId: string,
  projectOf: TabProjectLookup,
): string | null {
  let project: string | null = null;
  for (const tab of tabs) {
    if (tab.groupId !== groupId) continue;
    const key = projectKey(projectOf(tab.id));
    if (!project) project = key;
    else if (key && project !== key) return null;
  }
  return project;
}

/** Whether a tab may join an existing group. */
export function canJoinTabGroup<T extends GroupedTab>(
  tabs: T[],
  tabId: string,
  groupId: string,
  projectOf?: TabProjectLookup,
): boolean {
  if (!projectOf) return true;
  const project = tabGroupProject(tabs, groupId, projectOf);
  if (project == null) return true;
  return sameProject(project, projectKey(projectOf(tabId)));
}

/** Whether dropping one tab onto another may form or extend a group. */
export function canJoinTabOnto<T extends GroupedTab>(
  tabs: T[],
  draggedId: string,
  targetId: string,
  projectOf?: TabProjectLookup,
): boolean {
  if (draggedId === targetId) return false;
  if (!projectOf) return true;
  const dragged = tabs.find((tab) => tab.id === draggedId);
  const target = tabs.find((tab) => tab.id === targetId);
  if (!dragged || !target) return false;
  if (target.groupId) {
    return canJoinTabGroup(tabs, draggedId, target.groupId, projectOf);
  }
  if (dragged.groupId) {
    return canJoinTabGroup(tabs, targetId, dragged.groupId, projectOf);
  }
  return sameProject(
    projectKey(projectOf(draggedId)),
    projectKey(projectOf(targetId)),
  );
}

export function newTabGroupId(): string {
  return crypto.randomUUID();
}

export function sharedGroupProject(tabs: { project: string }[]): string | null {
  if (tabs.length === 0) return null;
  const first = tabs[0].project.trim();
  if (!first || first === "~") return null;
  return tabs.every((tab) => tab.project.trim() === first) ? first : null;
}

function withGroup<T extends GroupedTab>(tab: T, groupId: string | undefined): T {
  if (!groupId) {
    if (!tab.groupId) return tab;
    const next = { ...tab };
    delete next.groupId;
    return next;
  }
  if (tab.groupId === groupId) return tab;
  return { ...tab, groupId };
}

export function segmentTabs(tabs: Tab[]): TabGroupSegment[] {
  const segments: TabGroupSegment[] = [];
  let i = 0;

  while (i < tabs.length) {
    const tab = tabs[i];
    const groupId = tab.groupId?.trim();
    if (!groupId) {
      segments.push({ kind: "single", tab, index: i });
      i += 1;
      continue;
    }

    let j = i + 1;
    while (j < tabs.length && tabs[j].groupId === groupId) j += 1;
    const run = tabs.slice(i, j);
    segments.push({
      kind: "group",
      project: sharedGroupProject(run) ?? "",
      tabs: run,
      startIndex: i,
      color: tabGroupColor(groupId),
      key: groupId,
    });
    i = j;
  }

  return segments;
}

/** Move a whole segment (group or single tab) to a new position. */
export function reorderTabSegments(
  tabs: Tab[],
  fromSegmentIndex: number,
  toSegmentIndex: number,
): string[] | null {
  const segments = segmentTabs(tabs);
  if (
    fromSegmentIndex < 0 ||
    toSegmentIndex < 0 ||
    fromSegmentIndex >= segments.length ||
    toSegmentIndex >= segments.length
  ) {
    return null;
  }
  if (fromSegmentIndex === toSegmentIndex) {
    return tabs.map((tab) => tab.id);
  }

  const nextSegments = segments.slice();
  const [moved] = nextSegments.splice(fromSegmentIndex, 1);
  nextSegments.splice(toSegmentIndex, 0, moved);

  return nextSegments.flatMap((segment) =>
    segment.kind === "group"
      ? segment.tabs.map((tab) => tab.id)
      : [segment.tab.id],
  );
}

function permutationOf<T extends GroupedTab>(
  tabs: T[],
  orderedIds: string[],
): T[] | null {
  if (orderedIds.length !== tabs.length) return null;
  const byId = new Map(tabs.map((tab) => [tab.id, tab]));
  if (orderedIds.some((id) => !byId.has(id))) return null;
  return orderedIds.map((id) => byId.get(id)!);
}

/** Push a tab that landed inside a group it cannot join back out of the run. */
function slideOutOfGroup<T extends GroupedTab>(
  tabs: T[],
  ordered: T[],
  movedIndex: number,
  groupId: string,
): T[] {
  let start = movedIndex;
  while (start > 0 && ordered[start - 1].groupId === groupId) start -= 1;
  let end = movedIndex;
  while (end < ordered.length - 1 && ordered[end + 1].groupId === groupId) {
    end += 1;
  }

  const moved = ordered[movedIndex];
  const fromLeft =
    tabs.findIndex((tab) => tab.id === moved.id) <
    tabs.findIndex((tab) => tab.id === ordered[start].id);

  const next = ordered.slice();
  next.splice(movedIndex, 1);
  next.splice(fromLeft ? end : start, 0, withGroup(moved, undefined));
  return next;
}

/** Reorder tabs, joining a drop in the middle of a group and leaving when dragged out. */
export function applyGroupedReorder<T extends GroupedTab>(
  tabs: T[],
  orderedIds: string[],
  movedId: string,
  projectOf?: TabProjectLookup,
): T[] | null {
  const ordered = permutationOf(tabs, orderedIds);
  if (!ordered) return null;

  const movedIndex = ordered.findIndex((tab) => tab.id === movedId);
  if (movedIndex < 0) return ordered;

  const moved = ordered[movedIndex];
  const prev = ordered[movedIndex - 1];
  const next = ordered[movedIndex + 1];
  const prevGroup = prev?.groupId;
  const nextGroup = next?.groupId;

  let groupId = moved.groupId;
  if (prevGroup && prevGroup === nextGroup) {
    // A drop inside a run joins it, unless the group belongs to another
    // project — then the tab slides past the group instead of splitting it.
    if (!canJoinTabGroup(tabs, movedId, prevGroup, projectOf)) {
      return slideOutOfGroup(tabs, ordered, movedIndex, prevGroup);
    }
    groupId = prevGroup;
  } else if (
    groupId &&
    (prev?.groupId === groupId || next?.groupId === groupId)
  ) {
    // Keep membership when sliding along the edge of the same group.
  } else {
    groupId = undefined;
  }

  return ordered.map((tab) =>
    tab.id === movedId ? withGroup(tab, groupId) : tab,
  );
}

export function addTabToGroup<T extends GroupedTab>(
  tabs: T[],
  tabId: string,
  groupId: string,
  projectOf?: TabProjectLookup,
): T[] {
  const tab = tabs.find((entry) => entry.id === tabId);
  if (!tab) return tabs;
  if (!canJoinTabGroup(tabs, tabId, groupId, projectOf)) return tabs;
  const without = tabs.filter((entry) => entry.id !== tabId);
  const lastInGroup = without.reduce(
    (last, entry, index) => (entry.groupId === groupId ? index : last),
    -1,
  );
  const insertAt = lastInGroup >= 0 ? lastInGroup + 1 : without.length;
  const next = without.slice();
  next.splice(insertAt, 0, withGroup(tab, groupId));
  return next;
}

export function addTabsToNewGroup<T extends GroupedTab>(
  tabs: T[],
  tabIds: string[],
  groupId: string,
): T[] {
  const idSet = new Set(tabIds);
  const members = tabs
    .filter((tab) => idSet.has(tab.id))
    .map((tab) => withGroup(tab, groupId));
  if (members.length === 0) return tabs;
  const firstIndex = tabs.findIndex((tab) => idSet.has(tab.id));
  const rest = tabs.filter((tab) => !idSet.has(tab.id));
  rest.splice(Math.max(0, firstIndex), 0, ...members);
  return rest;
}

export function joinTabOnto<T extends GroupedTab>(
  tabs: T[],
  draggedId: string,
  targetId: string,
  createGroupId: () => string = newTabGroupId,
  projectOf?: TabProjectLookup,
): { tabs: T[]; groupId: string; created: boolean } | null {
  if (draggedId === targetId) return null;
  const dragged = tabs.find((tab) => tab.id === draggedId);
  const target = tabs.find((tab) => tab.id === targetId);
  if (!dragged || !target) return null;
  if (!canJoinTabOnto(tabs, draggedId, targetId, projectOf)) return null;

  const created = !target.groupId && !dragged.groupId;
  const groupId = target.groupId ?? dragged.groupId ?? createGroupId();

  const next = tabs.map((tab) => {
    if (tab.id === draggedId) return withGroup(tab, groupId);
    if (!target.groupId && tab.id === targetId) return withGroup(tab, groupId);
    return tab;
  });

  let dest = next.findIndex((tab) => tab.id === targetId);
  while (
    dest + 1 < next.length &&
    next[dest + 1].groupId === groupId &&
    next[dest + 1].id !== draggedId
  ) {
    dest += 1;
  }

  const from = next.findIndex((tab) => tab.id === draggedId);
  const [item] = next.splice(from, 1);
  if (from < dest) dest -= 1;
  next.splice(dest + 1, 0, item);
  return { tabs: next, groupId, created };
}

export function ungroupTabs<T extends GroupedTab>(tabs: T[], groupId: string): T[] {
  return tabs.map((tab) =>
    tab.groupId === groupId ? withGroup(tab, undefined) : tab,
  );
}

export function removeTabFromGroup<T extends GroupedTab>(
  tabs: T[],
  tabId: string,
): T[] {
  return tabs.map((tab) =>
    tab.id === tabId ? withGroup(tab, undefined) : tab,
  );
}

/** New tab inherits the active tab's group and sits beside it. */
export function insertTabBesideActive<T extends GroupedTab>(
  tabs: T[],
  tab: T,
  activeId: string | undefined,
  projectOf?: TabProjectLookup,
): T[] {
  if (!activeId) return [...tabs, tab];
  const activeIndex = tabs.findIndex((entry) => entry.id === activeId);
  if (activeIndex < 0) return [...tabs, tab];
  const active = tabs[activeIndex];
  const inherits =
    !!active.groupId &&
    canJoinTabGroup([...tabs, tab], tab.id, active.groupId, projectOf);
  const incoming = inherits ? withGroup(tab, active.groupId) : tab;
  const next = tabs.slice();
  next.splice(activeIndex + 1, 0, incoming);
  return next;
}

export function insertTabInGroup<T extends GroupedTab>(
  tabs: T[],
  tab: T,
  groupId: string,
): T[] {
  const without = tabs.filter((entry) => entry.id !== tab.id);
  return addTabToGroup([...without, tab], tab.id, groupId);
}

const COLLAPSED_KEY = "monocode:tab-groups:collapsed";

export function loadCollapsedTabGroups(): Set<string> {
  try {
    const raw = localStorage.getItem(COLLAPSED_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return new Set();
    return new Set(
      parsed.filter((entry): entry is string => typeof entry === "string"),
    );
  } catch {
    return new Set();
  }
}

export function saveCollapsedTabGroups(collapsed: Set<string>): void {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...collapsed]));
  } catch {
    /* ignore quota errors */
  }
}

export { replaceGroupInTabOrder } from "./workspaceTabGroups";
