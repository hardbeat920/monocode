import { sameProjectPath } from "../../projects/model/recents";

const MAX_PENDING_AGE_MS = 10 * 60_000;

export type InboxSelfActivityTarget = {
  provider: "github" | "gitlab" | "linear" | "jira" | "azuredevops";
  kind?: "issue" | "pr" | "linear" | "jira";
  repo?: string;
  number?: number;
  id?: string;
  projectPath?: string;
};

export type InboxSelfActivityItem = InboxSelfActivityTarget & {
  kind: "issue" | "pr" | "linear" | "jira";
  number: number;
  projectPath: string;
};

type PendingActivity = InboxSelfActivityTarget & { recordedAt: number };
type Listener = () => void;

const pending: PendingActivity[] = [];
const listeners = new Set<Listener>();

function normalized(value: string | undefined): string {
  return value?.trim().toLowerCase() ?? "";
}

function prune(now: number) {
  const oldest = now - MAX_PENDING_AGE_MS;
  for (let index = pending.length - 1; index >= 0; index -= 1) {
    if (pending[index]!.recordedAt < oldest) pending.splice(index, 1);
  }
}

function matches(activity: PendingActivity, item: InboxSelfActivityItem) {
  if (activity.provider !== item.provider) return false;
  if (activity.id && activity.id !== item.id) return false;
  if (activity.kind && activity.kind !== item.kind) return false;
  if (activity.number != null && activity.number !== item.number) return false;
  if (activity.repo && normalized(activity.repo) !== normalized(item.repo)) {
    return false;
  }
  if (
    activity.projectPath &&
    !sameProjectPath(activity.projectPath, item.projectPath)
  ) {
    return false;
  }
  return Boolean(
    activity.id ||
    activity.repo ||
    activity.number != null ||
    activity.projectPath,
  );
}

/** Remember a successful mutation so its resulting remote revision is not announced back to its author. */
export function recordInboxSelfActivity(
  target: InboxSelfActivityTarget,
  now = Date.now(),
) {
  prune(now);
  pending.push({ ...target, recordedAt: now });
  for (const listener of listeners) listener();
}

/** Consume all coalesced mutations for this revision. A later revision can notify normally. */
export function consumeInboxSelfActivity(
  item: InboxSelfActivityItem,
  now = Date.now(),
): boolean {
  prune(now);
  let consumed = false;
  for (let index = pending.length - 1; index >= 0; index -= 1) {
    if (!matches(pending[index]!, item)) continue;
    pending.splice(index, 1);
    consumed = true;
  }
  return consumed;
}

export function subscribeInboxSelfActivity(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function clearPendingInboxSelfActivity() {
  pending.splice(0, pending.length);
}
