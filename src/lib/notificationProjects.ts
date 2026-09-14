import { invoke } from "@tauri-apps/api/core";
import { pathKey, projectName } from "./paths";
import { looksLikeProject } from "./recents";
import type { InboxItem } from "./githubTasks";

export type NotificationProject = {
  id: string;
  name: string;
  detail: string;
  kind: "repository" | "local" | "linear";
  paths: string[];
};

type ProjectContext = {
  root: string;
  commonDir: string | null;
  remote: string | null;
};
const CATALOG_KEY = "monocode.notificationProjects.v1";
const CATALOG_CHANGE = "monocode:notification-projects-change";
const resolving = new Map<string, Promise<NotificationProject>>();
const queuedRefreshes = new Map<string, Promise<NotificationProject>>();
const REFRESH_AFTER_MS = 60_000;
const RETRY_AFTER_MS = 5_000;
const freshness = new Map<
  string,
  { id: string; checkedAt?: number; retryAfter?: number }
>();
let catalogValue: string | null | undefined;
let catalog: NotificationProject[] = [];
let catalogGeneration = 0;

function readCatalogValue(): string | null {
  const value = localStorage.getItem(CATALOG_KEY);
  if (value === null && catalogValue != null) {
    resolving.clear();
    queuedRefreshes.clear();
    freshness.clear();
    catalogGeneration++;
  }
  return value;
}

export function knownNotificationProject(
  path: string,
): NotificationProject | undefined {
  const key = pathKey(path);
  return loadNotificationProjects().find((project) =>
    project.paths.some((entry) => pathKey(entry) === key),
  );
}

/** A bulk action must never silently omit an unresolved local checkout. */
export function knownNotificationProjectSelection(
  paths: readonly string[],
): { projects: NotificationProject[] } | null {
  const projects = loadNotificationProjects();
  const knownPaths = new Set(
    projects.flatMap((project) => project.paths.map(pathKey)),
  );
  return paths
    .filter(looksLikeProject)
    .every((path) => knownPaths.has(pathKey(path)))
    ? { projects }
    : null;
}

export function loadNotificationProjects(): NotificationProject[] {
  try {
    const value = readCatalogValue();
    if (value === catalogValue) return catalog;
    catalogValue = value;
    catalog = [];
    const parsed: unknown = JSON.parse(value ?? "[]");
    if (!Array.isArray(parsed)) return (catalog = []);
    catalog = parsed.filter(
      (value): value is NotificationProject =>
        value &&
        typeof value.id === "string" &&
        typeof value.name === "string" &&
        typeof value.detail === "string" &&
        ["repository", "local", "linear"].includes(value.kind) &&
        Array.isArray(value.paths) &&
        value.paths.every((path: unknown) => typeof path === "string"),
    );
    return catalog;
  } catch {
    return catalog;
  }
}

export function rememberNotificationProjects(
  projects: readonly NotificationProject[],
) {
  storeNotificationProjects(projects, false);
}

function storeNotificationProjects(
  projects: readonly NotificationProject[],
  assignPaths: boolean,
) {
  const current = loadNotificationProjects();
  const byId = new Map(current.map((project) => [project.id, project]));
  for (const project of projects) {
    const paths = assignPaths
      ? project.paths
      : project.paths.filter(
          (path) =>
            ![...byId.values()].some(
              (entry) =>
                entry.id !== project.id &&
                entry.paths.some(
                  (knownPath) => pathKey(knownPath) === pathKey(path),
                ),
            ),
        );
    // A checkout can change remotes. Keep its former Inbox identity, but ensure
    // synchronous lookup associates the checkout only with its current project.
    const assignedPaths = new Set(paths.map(pathKey));
    for (const [id, entry] of byId) {
      if (
        id !== project.id &&
        entry.paths.some((path) => assignedPaths.has(pathKey(path)))
      ) {
        byId.set(id, {
          ...entry,
          paths: entry.paths.filter(
            (path) => !assignedPaths.has(pathKey(path)),
          ),
        });
      }
    }
    const previous = byId.get(project.id);
    byId.set(project.id, {
      ...project,
      paths: [...new Set([...(previous?.paths ?? []), ...paths])],
    });
  }
  const nextProjects = [...byId.values()];
  const next = JSON.stringify(nextProjects);
  if (next === JSON.stringify(current)) return;
  catalog = nextProjects;
  try {
    localStorage.setItem(CATALOG_KEY, next);
    catalogValue = next;
  } catch {
    // The catalog is an in-memory source of identity; persistence is only a cache.
  }
  window.dispatchEvent(new Event(CATALOG_CHANGE));
}

export function notificationProjectsSnapshot(): string {
  return JSON.stringify(loadNotificationProjects());
}

export function subscribeNotificationProjects(
  listener: () => void,
): () => void {
  const onStorage = (event: StorageEvent) => {
    if (event.key === CATALOG_KEY || event.key === null) listener();
  };
  window.addEventListener(CATALOG_CHANGE, listener);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(CATALOG_CHANGE, listener);
    window.removeEventListener("storage", onStorage);
  };
}

export async function resolveNotificationProject(
  path: string,
): Promise<NotificationProject> {
  const known = knownNotificationProject(path);
  if (known) {
    const previous = freshness.get(pathKey(path));
    const now = Date.now();
    if (
      !previous ||
      previous.id !== known.id ||
      ((previous.checkedAt === undefined ||
        now - previous.checkedAt >= REFRESH_AFTER_MS) &&
        now >= (previous.retryAfter ?? 0))
    ) {
      void refreshNotificationProject(path).catch(() => {});
    }
    return known;
  }
  return refreshNotificationProject(path);
}

function refreshNotificationProject(
  path: string,
): Promise<NotificationProject> {
  // Observe profile resets before reusing an in-flight lookup.
  loadNotificationProjects();
  const key = pathKey(path);
  const pending = resolving.get(key);
  if (pending) return pending;
  const generation = catalogGeneration;
  const promise = resolveLocalProject(path)
    .then((project) => {
      loadNotificationProjects();
      if (generation !== catalogGeneration) {
        throw new Error(
          "Notification project catalog changed during discovery",
        );
      }
      storeNotificationProjects([project], true);
      freshness.set(key, { id: project.id, checkedAt: Date.now() });
      return project;
    })
    .catch((error: unknown) => {
      const known = knownNotificationProject(path);
      if (known && generation === catalogGeneration) {
        const previous = freshness.get(key);
        freshness.set(key, {
          id: known.id,
          checkedAt: previous?.id === known.id ? previous.checkedAt : undefined,
          retryAfter: Date.now() + RETRY_AFTER_MS,
        });
      }
      throw error;
    })
    .finally(() => {
      if (resolving.get(key) === promise) resolving.delete(key);
    });
  resolving.set(key, promise);
  return promise;
}

/** Explicit invalidation after Git changes; ordinary reads use the freshness window. */
export async function refreshNotificationProjects(
  paths: readonly string[],
): Promise<void> {
  loadNotificationProjects();
  const uniquePaths = new Map(
    paths.filter(looksLikeProject).map((path) => [pathKey(path), path]),
  );
  await Promise.all(
    [...uniquePaths.values()].map(async (path) => {
      try {
        const key = pathKey(path);
        const pending = resolving.get(key);
        if (!pending) {
          await refreshNotificationProject(path);
        } else {
          let queued = queuedRefreshes.get(key);
          if (!queued) {
            const generation = catalogGeneration;
            queued = pending
              .catch(() => {})
              .then(() => {
                if (generation !== catalogGeneration)
                  throw new Error(
                    "Notification project catalog changed during discovery",
                  );
                // Further Git changes during this pass need another refresh.
                if (queuedRefreshes.get(key) === queued)
                  queuedRefreshes.delete(key);
                return refreshNotificationProject(path);
              })
              .finally(() => {
                if (queuedRefreshes.get(key) === queued)
                  queuedRefreshes.delete(key);
              });
            queuedRefreshes.set(key, queued);
          }
          await queued;
        }
      } catch (error) {
        if (!knownNotificationProject(path)) throw error;
      }
    }),
  );
}

async function resolveLocalProject(path: string): Promise<NotificationProject> {
  const context = await invoke<ProjectContext>("git_notification_context", {
    cwd: path,
  });
  const remote = repositoryProject(context.remote ?? "");
  if (remote) return { ...remote, paths: [path] };
  return {
    id: `local:${pathKey(context.commonDir ?? context.root)}`,
    name: projectName(context.root),
    detail: context.root,
    kind: "local",
    paths: [path],
  };
}

type NotificationWorkItem = Pick<InboxItem, "provider" | "repo" | "url"> &
  Partial<
    Pick<
      InboxItem,
      "projectPath" | "projectId" | "projectName" | "teamId" | "teamName"
    >
  >;

export function inboxNotificationProject(
  item: NotificationWorkItem,
): NotificationProject {
  if (item.provider === "linear") {
    const project = item.projectId?.trim();
    return {
      id: project
        ? `linear:project:${project}`
        : `linear:team:${item.teamId || "unknown"}:unassigned`,
      name: project ? item.projectName || project : "No project",
      detail: `Linear · ${item.teamName || item.teamId || "Unknown team"}`,
      kind: "linear",
      paths: [],
    };
  }
  const remote = repositoryProject(item.url, item.repo);
  if (remote)
    return { ...remote, paths: item.projectPath ? [item.projectPath] : [] };
  return {
    id: `local:${pathKey(item.projectPath || "~")}`,
    name: item.repo,
    detail: item.projectPath || "Unknown project",
    kind: "local",
    paths: item.projectPath ? [item.projectPath] : [],
  };
}

function repositoryProject(
  remote: string,
  repo?: string,
): NotificationProject | undefined {
  try {
    const scp = remote.match(/^(?:[^@/]+@)?([^/:]+):(.+)$/);
    const url = new URL(
      remote.includes("://")
        ? remote
        : scp
          ? `ssh://${scp[1]}/${scp[2]}`
          : remote,
    );
    if (!["http:", "https:", "ssh:", "git:"].includes(url.protocol)) return;
    const name = (repo ?? url.pathname)
      .replace(/^\/+|\/+$/g, "")
      .replace(/\.git$/, "");
    if (!url.hostname || !name.includes("/")) return;
    const host = url.host.toLowerCase();
    return {
      id: `repository:${host}/${name.toLowerCase()}`,
      name,
      detail: host,
      kind: "repository",
      paths: [],
    };
  } catch {
    return;
  }
}
