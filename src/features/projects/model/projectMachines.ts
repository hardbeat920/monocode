import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { pathKey } from "../../../shared/lib/paths";
import { IS_MAC, IS_WIN } from "../../../platform/tauri/platform";
import {
  isRemoteProjectPath,
  normalizeProjectPath,
  rememberProject,
  notifyProjectPathsChanged,
} from "./recents";
import { parseRemotePath } from "../../connections/model/remoteProjects";
import type { RemoteMachine } from "../../connections/model/protocol";

/**
 * One project, many machines. A rail project is a repository, and it may have
 * one folder (a location) on each machine: this computer and any paired host.
 * The home location stands for the project in the rail and keys its settings;
 * the other locations link to it. See docs/repo-machines.md.
 */
export type ProjectMachines = {
  /** Member location to its home location. Never chains. */
  links: Record<string, string>;
  /** Locations the user unlinked. Automatic linking leaves them alone. */
  separate: string[];
  /** Canonical repository key per location; "" when there is none. */
  identities: Record<string, string>;
};

const KEY = "monocode.projectMachines.v1";
export const PROJECT_MACHINES_CHANGED = "monocode:project-machines-changed";
export const LOCAL_MACHINE = "local";
export const THIS_COMPUTER = IS_MAC
  ? "This Mac"
  : IS_WIN
    ? "This PC"
    : "This computer";

const empty = (): ProjectMachines => ({ links: {}, separate: [], identities: {} });

const norm = (path: string) => normalizeProjectPath(path);
const same = (a: string, b: string) => pathKey(a) === pathKey(b);
const entryFor = (record: Record<string, string>, path: string) =>
  Object.entries(record).find(([key]) => same(key, path));

export function loadProjectMachines(): ProjectMachines {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "null") as
      | Partial<ProjectMachines>
      | null;
    if (!value || typeof value !== "object") return empty();
    const record = (input: unknown): Record<string, string> =>
      input && typeof input === "object" && !Array.isArray(input)
        ? Object.fromEntries(
            Object.entries(input).filter(
              (entry): entry is [string, string] => typeof entry[1] === "string",
            ),
          )
        : {};
    return {
      links: record(value.links),
      separate: Array.isArray(value.separate)
        ? value.separate.filter((path): path is string => typeof path === "string")
        : [],
      identities: record(value.identities),
    };
  } catch {
    return empty();
  }
}

function save(state: ProjectMachines): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    /* the links last until the app closes */
  }
  if (typeof window !== "undefined")
    window.dispatchEvent(new Event(PROJECT_MACHINES_CHANGED));
  notifyProjectPathsChanged();
}

/** `local` for this computer, else the environment id of the host. */
export function locationMachine(path: string): string {
  return isRemoteProjectPath(path)
    ? (parseRemotePath(path)?.environmentId ?? path)
    : LOCAL_MACHINE;
}

export const ADD_PROJECT_LOCATION_EVENT = "monocode:add-project-location";

export type AddProjectLocationRequest = {
  /** Any location of the project; the new folder joins its home. */
  project: string;
  where: "remote" | "local";
  /** A blank session to move to the new folder once it is added. */
  sessionId?: string;
};

/** Asks the app to add a folder for this project on another machine. */
export function requestProjectLocation(request: AddProjectLocationRequest): void {
  window.dispatchEvent(
    new CustomEvent<AddProjectLocationRequest>(ADD_PROJECT_LOCATION_EVENT, {
      detail: request,
    }),
  );
}

/** "This Mac" or the paired machine's name. */
export function locationLabel(
  path: string,
  machines: readonly Pick<RemoteMachine, "environmentId" | "name">[],
): string {
  const machine = locationMachine(path);
  if (machine === LOCAL_MACHINE) return THIS_COMPUTER;
  return (
    machines.find((entry) => entry.environmentId === machine)?.name ??
    "Another machine"
  );
}

/** The folder's path on its own machine. */
export function locationFolder(path: string): string {
  return parseRemotePath(path)?.hostPath ?? path;
}

export function homeIn(state: ProjectMachines, path: string): string {
  return entryFor(state.links, path)?.[1] ?? norm(path);
}

export function membersIn(state: ProjectMachines, home: string): string[] {
  return Object.entries(state.links)
    .filter(([member, to]) => !same(member, home) && same(to, home))
    .map(([member]) => member);
}

function withoutLink(links: Record<string, string>, path: string) {
  return Object.fromEntries(
    Object.entries(links).filter(([key]) => !same(key, path)),
  );
}

/** Home first, then the local member, then remote members in path order. */
export function locationsIn(state: ProjectMachines, path: string): string[] {
  const home = homeIn(state, path);
  const members = membersIn(state, home).sort((a, b) => {
    const localA = !isRemoteProjectPath(a);
    const localB = !isRemoteProjectPath(b);
    return localA !== localB ? (localA ? -1 : 1) : a.localeCompare(b);
  });
  return [home, ...members];
}

/** Links `member` (and anything linked to it) under `home`. False when the
 * project already has a folder on that machine. */
export function linkIn(
  state: ProjectMachines,
  home: string,
  member: string,
): ProjectMachines | undefined {
  const target = homeIn(state, home);
  if (same(target, member)) return undefined;
  const moving = [norm(member), ...membersIn(state, member)];
  const taken = new Set(locationsIn(state, target).map(locationMachine));
  for (const path of moving) {
    const machine = locationMachine(path);
    if (taken.has(machine)) return undefined;
    taken.add(machine);
  }
  let links = state.links;
  for (const path of moving) links = { ...withoutLink(links, path), [path]: target };
  return {
    ...state,
    links,
    separate: state.separate.filter(
      (path) => !moving.some((entry) => same(entry, path)),
    ),
  };
}

/** Removes `path` from its project. A removed home hands the project to its
 * first remaining location. */
function detachIn(state: ProjectMachines, path: string): ProjectMachines {
  const home = homeIn(state, path);
  if (!same(home, path))
    return { ...state, links: withoutLink(state.links, path) };
  const [next, ...rest] = locationsIn(state, home).slice(1);
  if (!next) return state;
  const links = withoutLink(state.links, next);
  for (const member of rest) links[member] = next;
  return { ...state, links };
}

export function unlinkIn(state: ProjectMachines, path: string): ProjectMachines {
  const next = detachIn(state, path);
  return next.separate.some((entry) => same(entry, path))
    ? next
    : { ...next, separate: [...next.separate, norm(path)] };
}

export function forgetIn(state: ProjectMachines, path: string): ProjectMachines {
  const next = detachIn(state, path);
  return {
    ...next,
    separate: next.separate.filter((entry) => !same(entry, path)),
    identities: withoutLink(next.identities, path),
  };
}

export const projectHome = (path: string) => homeIn(loadProjectMachines(), path);
export const projectLocations = (path: string) =>
  locationsIn(loadProjectMachines(), path);
export const isLinkedMember = (path: string) =>
  !same(projectHome(path), path);

export function linkProjectLocation(home: string, member: string): boolean {
  const next = linkIn(loadProjectMachines(), home, member);
  if (!next) return false;
  save(next);
  return true;
}

/** Splits a location back out into its own rail project. */
export function unlinkProjectLocation(path: string): void {
  save(unlinkIn(loadProjectMachines(), path));
  rememberProject(path);
}

/** The project left the rail; its location stops belonging to anything. */
export function forgetMachineLocation(path: string): void {
  const state = loadProjectMachines();
  const next = forgetIn(state, path);
  if (JSON.stringify(next) !== JSON.stringify(state)) save(next);
}

/** A number that changes whenever project links change. */
export function useProjectMachinesRevision(): number {
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const changed = () => setRevision((value) => value + 1);
    window.addEventListener(PROJECT_MACHINES_CHANGED, changed);
    return () => window.removeEventListener(PROJECT_MACHINES_CHANGED, changed);
  }, []);
  return revision;
}

/** This path's locations, kept current as links change. */
export function useProjectLocations(path: string): string[] {
  const revision = useProjectMachinesRevision();
  const key = path && path !== "~" ? path : "";
  return useMemo(() => (key ? projectLocations(key) : []), [key, revision]);
}

/**
 * A repository's identity across machines, from one of its remote URLs.
 * `git@github.com:a/b.git` and `https://github.com/a/b` agree. The Rust app
 * implements the same rules; the shared vectors live in docs/repo-machines.md.
 */
export function normalizeGitRemoteUrl(url: string): string {
  let value = url.trim();
  if (!value) return "";
  value = value.replace(/\/+$/, "").replace(/\.git$/i, "").replace(/\/+$/, "");
  const scheme = /^[A-Za-z][A-Za-z0-9+.-]*:\/\//.exec(value);
  let result: string;
  if (scheme) {
    const rest = value.slice(scheme[0].length);
    const slash = rest.indexOf("/");
    const authority = slash < 0 ? rest : rest.slice(0, slash);
    const path = slash < 0 ? "" : rest.slice(slash);
    const host = authority.slice(authority.lastIndexOf("@") + 1).replace(/:\d*$/, "");
    result = `${host}${path}`;
  } else {
    const scp = /^(?:[^@/\\]+@)?([^:/\\]{2,}):(.+)$/.exec(value);
    result = scp
      ? `${scp[1]}/${scp[2].replace(/^\/+/, "")}`
      : `file:${value.replace(/\\/g, "/")}`;
  }
  const azureSsh = /^ssh\.dev\.azure\.com\/v3\/([^/]+)\/([^/]+)\/([^/]+)$/i.exec(result);
  if (azureSsh)
    result = `dev.azure.com/${azureSsh[1]}/${azureSsh[2]}/_git/${azureSsh[3]}`;
  const visualStudio = /^([^/.]+)\.visualstudio\.com\/(.+)$/i.exec(result);
  if (visualStudio) result = `dev.azure.com/${visualStudio[1]}/${visualStudio[2]}`;
  return result.toLowerCase();
}

export async function localRemoteUrl(cwd: string): Promise<string | undefined> {
  try {
    return (await invoke<string | null>("git_remote_url", { cwd })) ?? undefined;
  } catch {
    return undefined;
  }
}

type Candidate = { path: string; openedAt: number };

/**
 * Groups rail projects that share a repository and live on different
 * machines. Each group joins the home that already has the most locations
 * (this computer, then the oldest, breaks ties). A machine keeps at most one
 * location per project: its most recently opened folder.
 */
export function autoLinkIn(
  state: ProjectMachines,
  railProjects: readonly Candidate[],
): ProjectMachines {
  const groups = new Map<string, Candidate[]>();
  for (const project of railProjects) {
    const identity = entryFor(state.identities, project.path)?.[1];
    if (
      !identity ||
      state.separate.some((entry) => same(entry, project.path)) ||
      entryFor(state.links, project.path)
    )
      continue;
    groups.set(identity, [...(groups.get(identity) ?? []), project]);
  }
  let next = state;
  for (const group of groups.values()) {
    if (group.length < 2) continue;
    const ranked = [...group].sort((a, b) => {
      const size =
        locationsIn(next, b.path).length - locationsIn(next, a.path).length;
      if (size) return size;
      const localA = !isRemoteProjectPath(a.path);
      const localB = !isRemoteProjectPath(b.path);
      if (localA !== localB) return localA ? -1 : 1;
      return a.openedAt - b.openedAt;
    });
    const home = ranked[0].path;
    const others = ranked.slice(1).sort((a, b) => b.openedAt - a.openedAt);
    for (const other of others) next = linkIn(next, home, other.path) ?? next;
  }
  return next;
}

/** Records identities, then links what matches. True when links changed. */
export function autoLinkProjects(
  railProjects: readonly Candidate[],
  identities: Record<string, string | undefined>,
): boolean {
  const state = loadProjectMachines();
  let known = state.identities;
  for (const [path, url] of Object.entries(identities))
    known = {
      ...withoutLink(known, path),
      [norm(path)]: url ? normalizeGitRemoteUrl(url) : "",
    };
  const next = autoLinkIn({ ...state, identities: known }, railProjects);
  if (JSON.stringify(next) === JSON.stringify(state)) return false;
  const linked = JSON.stringify(next.links) !== JSON.stringify(state.links);
  if (linked) save(next);
  else
    try {
      localStorage.setItem(KEY, JSON.stringify(next));
    } catch {
      /* identities are looked up again next launch */
    }
  return linked;
}

/** Locations whose identity has not been looked up yet. */
export function unknownIdentities(paths: readonly string[]): string[] {
  const { identities } = loadProjectMachines();
  return paths.filter((path) => !entryFor(identities, path));
}
