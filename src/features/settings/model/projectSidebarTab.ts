import { pathKey } from "../../../shared/lib/paths";
import { loadSidebarTabOrder, type SidebarTabId } from "./appearance";

const KEY = "monocode.projectSidebarTabs.v1";

type ProjectSidebarTab = Exclude<SidebarTabId, "inbox">;
type StoredTabs = Record<string, ProjectSidebarTab>;

function isProjectSidebarTab(value: unknown): value is ProjectSidebarTab {
  return value === "sessions" || value === "files" || value === "changes";
}

function readAll(): StoredTabs {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
      return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(([, tab]) => isProjectSidebarTab(tab)),
    );
  } catch {
    return {};
  }
}

function writeAll(tabs: StoredTabs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(tabs));
  } catch {
    // private mode / quota
  }
}

export function loadProjectSidebarTab(project: string): ProjectSidebarTab {
  const saved = readAll()[pathKey(project)];
  return saved ?? loadSidebarTabOrder().find(isProjectSidebarTab) ?? "sessions";
}

export function saveProjectSidebarTab(
  project: string,
  tab: SidebarTabId,
): void {
  if (project === "~" || !isProjectSidebarTab(tab)) return;
  writeAll({ ...readAll(), [pathKey(project)]: tab });
}

export function clearProjectSidebarTab(project: string): void {
  const tabs = readAll();
  const key = pathKey(project);
  if (!Object.prototype.hasOwnProperty.call(tabs, key)) return;
  delete tabs[key];
  writeAll(tabs);
}

export function rebaseProjectSidebarTab(from: string, to: string): void {
  const oldKey = pathKey(from);
  const newKey = pathKey(to);
  if (oldKey === newKey) return;
  const tabs = readAll();
  if (!Object.prototype.hasOwnProperty.call(tabs, oldKey)) return;
  tabs[newKey] = tabs[oldKey];
  delete tabs[oldKey];
  writeAll(tabs);
}
