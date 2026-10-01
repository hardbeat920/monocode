import { invoke } from "@tauri-apps/api/core";
import {
  automationTriggers,
  listAutomations,
  notifyAutomationsChanged,
  type Automation,
  type AutomationTrigger,
  type AutomationTriggerKind,
  type DueAutomationRun,
} from "./automations";
import { inboxStartDraft, type InboxItem } from "../../inbox/model/githubTasks";
import type {
  InboxTransition,
  InboxTransitionKind,
} from "../../inbox/model/inboxTransitions";
import { sameProjectPath } from "../../projects/model/recents";
import type { LinkedWorkItem } from "../../sessions/model/session";
import { linkedWorkItemFromInboxItem } from "../../sessions/model/sessionWorkItem";

export type InboxAutomationMatch = {
  automation: Automation;
  trigger: AutomationTrigger;
  item: InboxItem;
  eventKey: string;
  /** Key of the observed event this came from, which is what gets retried. */
  sourceKey: string;
  occurredAt: number;
  prompt: string;
};

export type ClaimedInboxAutomationRun = DueAutomationRun & {
  prompt: string;
  linkedWorkItem?: LinkedWorkItem;
};

/** An Inbox item that appeared, or one that went through `transition`. */
type InboxEvent = {
  item: InboxItem;
  transition?: InboxTransitionKind;
  label?: string;
};

const RETRY_STORAGE_KEY = "monocode.automation-inbox-retries.v1";
const MAX_RETRY_ITEMS = 500;
let retryItems: Map<string, InboxEvent> | undefined;

const TRANSITION_EVENTS: Record<
  "issue" | "pr",
  Partial<Record<InboxTransitionKind, string>>
> = {
  issue: {
    reopened: "issue_reopened",
    closed: "issue_closed",
    labeled: "issue_labeled",
  },
  pr: {
    reopened: "pull_request_reopened",
    closed: "pull_request_closed",
    merged: "pull_request_merged",
    ready_for_review: "pull_request_ready_for_review",
    labeled: "pull_request_labeled",
  },
};

const TRANSITION_PHRASES: Record<InboxTransitionKind, string> = {
  reopened: "was reopened",
  closed: "was closed",
  merged: "was merged",
  ready_for_review: "was marked ready for review",
  labeled: "was labeled",
};

export const SUPPORTED_INBOX_TRIGGER_EVENTS = {
  github: [
    "draft_opened",
    "pull_request_opened",
    ...Object.values(TRANSITION_EVENTS.pr),
    "issue_opened",
    ...Object.values(TRANSITION_EVENTS.issue),
  ],
  gitlab: ["merge_request_opened", "issue_opened"],
  linear: ["issue_created"],
  jira: ["issue_created"],
  azuredevops: ["pull_request_appeared", "work_item_appeared"],
} as const;

export function inboxAppearedEvent(
  item: InboxItem,
): { kind: AutomationTriggerKind; event: string } | null {
  if (item.provider === "github" && item.kind === "pr") {
    return {
      kind: "github",
      event: item.draft ? "draft_opened" : "pull_request_opened",
    };
  }
  if (item.provider === "github" && item.kind === "issue") {
    return { kind: "github", event: "issue_opened" };
  }
  if (item.provider === "gitlab" && item.kind === "pr") {
    return { kind: "gitlab", event: "merge_request_opened" };
  }
  if (item.provider === "gitlab" && item.kind === "issue") {
    return { kind: "gitlab", event: "issue_opened" };
  }
  if (item.provider === "linear" || item.provider === "jira") {
    return { kind: item.provider, event: "issue_created" };
  }
  if (item.provider === "azuredevops" && item.kind === "pr") {
    return { kind: "azuredevops", event: "pull_request_appeared" };
  }
  if (item.provider === "azuredevops" && item.kind === "issue") {
    return { kind: "azuredevops", event: "work_item_appeared" };
  }
  return null;
}

export function inboxTransitionEvent(
  item: InboxItem,
  transition: InboxTransitionKind,
): { kind: AutomationTriggerKind; event: string } | null {
  if (item.provider !== "github") return null;
  if (item.kind !== "issue" && item.kind !== "pr") return null;
  const event = TRANSITION_EVENTS[item.kind][transition];
  return event ? { kind: "github", event } : null;
}

/**
 * An item opens once, so its key is the item alone. Later changes can repeat,
 * so each carries the change, the label when one was added, and when it
 * happened.
 */
export function automationEventKey(
  item: InboxItem,
  transition?: InboxTransitionKind,
  label?: string,
): string {
  const slug = label
    ?.trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]/g, "_");
  const base =
    item.provider === "linear" || item.provider === "jira"
      ? `${item.provider}:issue:${item.id || item.identifier || item.number}`
      : `${item.provider}:${item.kind}:${item.repo}:${item.number}`;
  const identity = transition
    ? `${base}:${transition}${slug ? `:${slug}` : ""}:${Date.parse(item.updatedAt) || 0}`
    : base;
  return identity
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9:_\-./]/g, "_")
    .slice(0, 400);
}

function inboxEventKey(event: InboxEvent): string {
  return automationEventKey(event.item, event.transition, event.label);
}

function savedInboxEvent(saved: unknown): InboxEvent | null {
  if (!saved || typeof saved !== "object") return null;
  const entry = saved as Partial<InboxTransition>;
  // Appeared items are stored bare, as they were before transitions existed.
  if (!entry.transition) return { item: saved as InboxItem };
  if (!entry.item || !(entry.transition in TRANSITION_PHRASES)) return null;
  return {
    item: entry.item,
    transition: entry.transition,
    ...(typeof entry.label === "string" ? { label: entry.label } : {}),
  };
}

function pendingRetryItems(): Map<string, InboxEvent> {
  if (retryItems) return retryItems;
  retryItems = new Map();
  try {
    const saved = JSON.parse(window.localStorage.getItem(RETRY_STORAGE_KEY) ?? "[]");
    if (Array.isArray(saved)) {
      for (const entry of saved) {
        const event = savedInboxEvent(entry);
        if (!event) continue;
        const key = inboxEventKey(event);
        if (key) retryItems.set(key, event);
      }
    }
  } catch {
    // A malformed retry cache should not block new Inbox events.
  }
  return retryItems;
}

function saveRetryItems(items: ReadonlyMap<string, InboxEvent>) {
  try {
    if (items.size === 0) window.localStorage.removeItem(RETRY_STORAGE_KEY);
    else
      window.localStorage.setItem(
        RETRY_STORAGE_KEY,
        JSON.stringify(
          [...items.values()].map((event) =>
            event.transition ? event : event.item,
          ),
        ),
      );
  } catch {
    // The in-memory queue still retries while storage is unavailable.
  }
}

function inboxTransitionDraft(
  item: InboxItem,
  transition: InboxTransitionKind,
  label?: string,
): string {
  const [, ...details] = inboxStartDraft(item).trim().split("\n");
  const kind = item.kind === "pr" ? "pull request" : "issue";
  const what = `${TRANSITION_PHRASES[transition]}${label ? ` "${label}"` : ""}`;
  return [`This GitHub ${kind} ${what}:`, ...details].join("\n");
}

export function matchInboxAutomations(
  automations: readonly Automation[],
  appeared: readonly InboxItem[],
  transitions: readonly InboxTransition[] = [],
): InboxAutomationMatch[] {
  const matches: InboxAutomationMatch[] = [];
  const seen = new Set<string>();
  const events: Array<InboxEvent & { sourceKey: string }> = [];
  for (const item of appeared) {
    const sourceKey = automationEventKey(item);
    events.push({ item, sourceKey });
    // The labels on a new item were added when it was created.
    for (const label of item.labels) {
      events.push({ item, transition: "labeled", label: label.name, sourceKey });
    }
  }
  for (const transition of transitions) {
    events.push({ ...transition, sourceKey: inboxEventKey(transition) });
  }
  for (const { item, transition, label, sourceKey } of events) {
    const event = transition
      ? inboxTransitionEvent(item, transition)
      : inboxAppearedEvent(item);
    if (!event || !sourceKey) continue;
    for (const automation of automations) {
      if (!automation.enabled) continue;
      const trigger = automationTriggers(automation).find((candidate) =>
        triggerMatchesInboxItem(candidate, automation.cwd, item, event, label),
      );
      if (!trigger) continue;
      // A trigger for any label runs once for labels added together.
      const eventKey = automationEventKey(
        item,
        transition,
        triggerLabel(trigger) ? label : undefined,
      );
      // One run per automation for each event, and for each observed change:
      // a new item that also satisfies a label trigger still runs once.
      const byEvent = `${automation.id}:${eventKey}`;
      const bySource = `${automation.id}:${sourceKey}`;
      if (seen.has(byEvent) || seen.has(bySource)) continue;
      seen.add(byEvent);
      seen.add(bySource);
      matches.push({
        automation,
        trigger,
        item,
        eventKey,
        sourceKey,
        occurredAt: transition
          ? Date.parse(item.updatedAt) || 0
          : itemOccurredAt(item),
        prompt: `${automation.prompt.trim()}\n\n${
          transition
            ? inboxTransitionDraft(item, transition, label)
            : inboxStartDraft(item).trim()
        }`,
      });
    }
  }
  return matches;
}

export async function claimInboxAutomationRuns(
  appeared: readonly InboxItem[],
  now = Date.now(),
  transitions: readonly InboxTransition[] = [],
): Promise<ClaimedInboxAutomationRun[]> {
  const pending = pendingRetryItems();
  for (const event of [
    ...appeared.map((item): InboxEvent => ({ item })),
    ...transitions,
  ]) {
    const key = inboxEventKey(event);
    if (key) pending.set(key, event);
  }
  while (pending.size > MAX_RETRY_ITEMS) {
    const oldest = pending.keys().next().value;
    if (oldest == null) break;
    pending.delete(oldest);
  }
  saveRetryItems(pending);
  if (pending.size === 0) return [];

  const automations = await listAutomations();
  const candidates = [...pending.values()];
  const matches = matchInboxAutomations(
    automations,
    candidates.filter((event) => !event.transition).map((event) => event.item),
    candidates.filter(
      (event): event is InboxTransition => event.transition !== undefined,
    ),
  );
  const claimed: ClaimedInboxAutomationRun[] = [];
  const failed = new Set<string>();
  for (const match of matches) {
    try {
      const result = await invoke<DueAutomationRun | null>(
        "automations_claim_event",
        {
          automationId: match.automation.id,
          claim: {
            eventKey: match.eventKey,
            eventKind: match.trigger.kind,
            event: match.trigger.event,
            scheduledFor: match.occurredAt || now,
            prompt: match.prompt,
          },
          now,
        },
      );
      if (result) {
        const linkedWorkItem = linkedWorkItemFromInboxItem(match.item);
        claimed.push({
          ...result,
          prompt: match.prompt,
          ...(linkedWorkItem ? { linkedWorkItem } : {}),
        });
      }
    } catch {
      failed.add(match.sourceKey);
    }
  }
  for (const event of candidates) {
    const key = inboxEventKey(event);
    if (!failed.has(key)) pending.delete(key);
  }
  saveRetryItems(pending);
  if (claimed.length > 0) notifyAutomationsChanged();
  return claimed;
}

function triggerLabel(trigger: AutomationTrigger): string {
  return (trigger.label ?? "").trim().toLowerCase();
}

function triggerMatchesInboxItem(
  trigger: AutomationTrigger,
  cwd: string,
  item: InboxItem,
  event: { kind: AutomationTriggerKind; event: string },
  label?: string,
): boolean {
  if (trigger.kind !== event.kind || trigger.event !== event.event) return false;
  const wanted = triggerLabel(trigger);
  if (wanted && wanted !== label?.trim().toLowerCase()) return false;
  if (!matchesInboxProject(item, cwd)) return false;
  if (!matchesActor(trigger.actor)) return false;
  const repos = [...trigger.repos, trigger.repo]
    .map((repo) => repo.trim().toLowerCase())
    .filter(Boolean);
  if (repos.length === 0) return true;
  return repos.includes(item.repo.trim().toLowerCase());
}

function matchesInboxProject(item: InboxItem, cwd: string): boolean {
  // Linear and Jira issues have no git path. The automation's own
  // project is the workspace the agent should run in.
  if (
    (item.provider === "linear" || item.provider === "jira") &&
    !item.projectPath.trim()
  ) return true;
  return sameProjectPath(item.projectPath, cwd);
}

function matchesActor(actor: string): boolean {
  const value = actor.trim().toLowerCase();
  return value.length === 0 || value === "anyone";
}

function itemOccurredAt(item: InboxItem): number {
  const created = item.createdAt ? Date.parse(item.createdAt) : Number.NaN;
  if (Number.isFinite(created)) return created;
  const updated = Date.parse(item.updatedAt);
  return Number.isFinite(updated) ? updated : 0;
}
