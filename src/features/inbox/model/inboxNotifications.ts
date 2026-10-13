import type { InboxItem, InboxProvider } from "./githubTasks";
import { inboxNotificationProject } from "../../notifications/model/notificationProjects";
import type { NotificationSubject } from "../../notifications/model/notificationPreferences";

export function inboxNotificationSubject(
  item: Parameters<typeof inboxNotificationProject>[0] &
    Pick<InboxItem, "kind" | "updatedAt">,
): NotificationSubject {
  return {
    projectId: inboxNotificationProject(item).id,
    category: item.kind === "pr" ? "pullRequests" : "issues",
    occurredAt: Date.parse(item.updatedAt),
  };
}

export type InboxObservation = {
  changed: InboxItem[];
  appeared: InboxItem[];
};

/**
 * Stable identity for an item across sessions and query scopes. Work items
 * keyed by their local project path re-appear as brand-new whenever a project
 * is added or its scope changes, which made event automations replay every
 * open item (e.g. review all old PRs) after a baseline change. Remote
 * providers are identified by the hosting repo/team instead, so items only
 * "appear" when they are new upstream.
 */
function observationKey(item: InboxItem): string {
  if (item.provider === "jira") {
    return `jira:${item.teamId || item.repo.toLowerCase()}:${item.id || item.identifier || item.number}`;
  }
  if (item.provider === "linear") {
    return `linear:${item.projectId || item.teamId || "unknown"}:${item.id || item.identifier || item.number}`;
  }
  if (item.provider === "azuredevops") {
    const remote = remoteRepoIdentity(item.url, item.repo);
    return `azuredevops:${remote}:${item.kind}:${item.id || item.number}`;
  }
  const remote = remoteRepoIdentity(item.url, item.repo);
  return `${item.provider}:${remote}:${item.kind}:${item.id || item.number}`;
}

function remoteRepoIdentity(url: string, repo: string | undefined): string {
  const fallback = (repo ?? "unknown").trim().toLowerCase();
  try {
    const parsed = new URL(url);
    const segments = parsed.pathname
      .replace(/\/+$/, "")
      .split("/")
      .filter(Boolean);
    if (segments.length === 0) return fallback;
    // Azure DevOps URLs are /org/project/_git/repo; item URLs elsewhere are
    // /owner/repo/pull|issues/N, so the repository is the leading segments.
    const gitIndex = segments.indexOf("_git");
    const nameSegments =
      gitIndex >= 0 ? segments.slice(0, gitIndex + 2) : segments.slice(0, 2);
    return `${parsed.host.toLowerCase()}/${nameSegments.join("/")}`.toLowerCase();
  } catch {
    return fallback;
  }
}

/** Tracks fetched revisions separately from the user's read/unread state. */
export class InboxNotificationTracker {
  private revisions = new Map<string, number>();
  private primed = new Set<InboxProvider>();
  private scope: string | undefined;

  observe(
    items: readonly InboxItem[],
    scope: string,
    failedProviders: readonly InboxProvider[] = [],
  ): InboxObservation {
    if (scope !== this.scope) this.primed.clear();
    this.scope = scope;
    const failed = new Set(failedProviders);
    const changed: InboxItem[] = [];
    const appeared: InboxItem[] = [];
    for (const item of items) {
      if (failed.has(item.provider)) continue;
      const key = observationKey(item);
      const updatedAt = Date.parse(item.updatedAt);
      if (!Number.isFinite(updatedAt)) continue;
      const previous = this.revisions.get(key);
      if (this.primed.has(item.provider) && previous === undefined) {
        appeared.push(item);
        changed.push(item);
      } else if (
        this.primed.has(item.provider) &&
        previous !== undefined &&
        updatedAt > previous
      ) {
        changed.push(item);
      }
      this.revisions.set(key, Math.max(previous ?? 0, updatedAt));
    }
    for (const provider of [
      "github",
      "gitlab",
      "linear",
      "jira",
      "azuredevops",
    ] as const) {
      if (!failed.has(provider)) this.primed.add(provider);
    }
    return { changed, appeared };
  }
}
