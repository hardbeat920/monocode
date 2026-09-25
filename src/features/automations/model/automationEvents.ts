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
import { sameProjectPath } from "../../projects/model/recents";
import type { LinkedWorkItem } from "../../sessions/model/session";
import { linkedWorkItemFromInboxItem } from "../../sessions/model/sessionWorkItem";

export type InboxAutomationMatch = {
  automation: Automation;
  trigger: AutomationTrigger;
  item: InboxItem;
  eventKey: string;
  occurredAt: number;
  prompt: string;
};

export type ClaimedInboxAutomationRun = DueAutomationRun & {
  prompt: string;
  linkedWorkItem?: LinkedWorkItem;
};

const RETRY_STORAGE_KEY = "monocode.automation-inbox-retries.v1";
const MAX_RETRY_ITEMS = 500;
let retryItems: Map<string, InboxItem> | undefined;

export const SUPPORTED_INBOX_TRIGGER_EVENTS = {
  github: ["draft_opened", "pull_request_opened", "issue_opened"],
  gitlab: ["merge_request_opened", "issue_opened"],
  linear: ["issue_created"],
  jira: ["issue_created"],
  asana: ["issue_created"],
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
  if (
    item.provider === "linear" ||
    item.provider === "jira" ||
    item.provider === "asana"
  ) {
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

export function automationEventKey(item: InboxItem): string {
  const identity =
    item.provider === "linear" || item.provider === "jira"
      ? `${item.provider}:issue:${item.id || item.identifier || item.number}`
      : item.provider === "asana"
        ? `asana:task:${item.id || item.identifier || item.number}`
        : `${item.provider}:${item.kind}:${item.repo}:${item.number}`;
  return identity
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9:_\-./]/g, "_")
    .slice(0, 400);
}

function pendingRetryItems(): Map<string, InboxItem> {
  if (retryItems) return retryItems;
  retryItems = new Map();
  try {
    const saved = JSON.parse(window.localStorage.getItem(RETRY_STORAGE_KEY) ?? "[]");
    if (Array.isArray(saved)) {
      for (const item of saved as InboxItem[]) {
        const key = automationEventKey(item);
        if (key) retryItems.set(key, item);
      }
    }
  } catch {
    // A malformed retry cache should not block new Inbox events.
  }
  return retryItems;
}

function saveRetryItems(items: ReadonlyMap<string, InboxItem>) {
  try {
    if (items.size === 0) window.localStorage.removeItem(RETRY_STORAGE_KEY);
    else
      window.localStorage.setItem(
        RETRY_STORAGE_KEY,
        JSON.stringify([...items.values()]),
      );
  } catch {
    // The in-memory queue still retries while storage is unavailable.
  }
}

export function matchInboxAutomations(
  automations: readonly Automation[],
  appeared: readonly InboxItem[],
): InboxAutomationMatch[] {
  const matches: InboxAutomationMatch[] = [];
  const seen = new Set<string>();
  for (const item of appeared) {
    const event = inboxAppearedEvent(item);
    if (!event) continue;
    const eventKey = automationEventKey(item);
    if (!eventKey) continue;
    for (const automation of automations) {
      if (!automation.enabled) continue;
      const trigger = automationTriggers(automation).find((candidate) =>
        triggerMatchesInboxItem(candidate, automation.cwd, item, event),
      );
      if (!trigger) continue;
      const dedupe = `${automation.id}:${eventKey}`;
      if (seen.has(dedupe)) continue;
      seen.add(dedupe);
      matches.push({
        automation,
        trigger,
        item,
        eventKey,
        occurredAt: itemOccurredAt(item),
        prompt: `${automation.prompt.trim()}\n\n${inboxStartDraft(item).trim()}`,
      });
    }
  }
  return matches;
}

export async function claimInboxAutomationRuns(
  appeared: readonly InboxItem[],
  now = Date.now(),
): Promise<ClaimedInboxAutomationRun[]> {
  const pending = pendingRetryItems();
  for (const item of appeared) {
    const key = automationEventKey(item);
    if (key) pending.set(key, item);
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
  const matches = matchInboxAutomations(automations, candidates);
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
      failed.add(match.eventKey);
    }
  }
  for (const item of candidates) {
    const key = automationEventKey(item);
    if (!failed.has(key)) pending.delete(key);
  }
  saveRetryItems(pending);
  if (claimed.length > 0) notifyAutomationsChanged();
  return claimed;
}

function triggerMatchesInboxItem(
  trigger: AutomationTrigger,
  cwd: string,
  item: InboxItem,
  event: { kind: AutomationTriggerKind; event: string },
): boolean {
  if (trigger.kind !== event.kind || trigger.event !== event.event) return false;
  if (!matchesInboxProject(item, cwd)) return false;
  if (!matchesActor(trigger.actor)) return false;
  const repos = [...trigger.repos, trigger.repo]
    .map((repo) => repo.trim().toLowerCase())
    .filter(Boolean);
  if (repos.length === 0) return true;
  return repos.includes(item.repo.trim().toLowerCase());
}

function matchesInboxProject(item: InboxItem, cwd: string): boolean {
  // Linear, Jira and Asana items have no git path. The automation's own
  // project is the workspace the agent should run in.
  if (
    (item.provider === "linear" ||
      item.provider === "jira" ||
      item.provider === "asana") &&
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
