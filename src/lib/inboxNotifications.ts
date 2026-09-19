import type { InboxItem, InboxProvider } from "./githubTasks";
import { inboxNotificationProject } from "./notificationProjects";
import type { NotificationSubject } from "./notificationPreferences";

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

/** Tracks fetched revisions separately from the user's read/unread state. */
export class InboxNotificationTracker {
  private revisions = new Map<string, number>();
  private primed = new Set<InboxProvider>();
  private scope: string | undefined;

  observe(
    items: readonly InboxItem[],
    scope: string,
    failedProviders: readonly InboxProvider[] = [],
  ): InboxItem[] {
    if (scope !== this.scope) this.primed.clear();
    this.scope = scope;
    const failed = new Set(failedProviders);
    const changed: InboxItem[] = [];
    for (const item of items) {
      if (failed.has(item.provider)) continue;
      const key = JSON.stringify([
        inboxNotificationProject(item).id,
        item.kind,
        item.id || item.number,
      ]);
      const updatedAt = Date.parse(item.updatedAt);
      if (!Number.isFinite(updatedAt)) continue;
      const previous = this.revisions.get(key);
      if (
        this.primed.has(item.provider) &&
        (previous === undefined || updatedAt > previous)
      )
        changed.push(item);
      this.revisions.set(key, Math.max(previous ?? 0, updatedAt));
    }
    for (const provider of [
      "github",
      "gitlab",
      "linear",
      "azuredevops",
    ] as const) {
      if (!failed.has(provider)) this.primed.add(provider);
    }
    return changed;
  }
}
