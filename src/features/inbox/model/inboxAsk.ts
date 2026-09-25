import type { InboxItem } from "./githubTasks";
import inboxInstructions from "../../../instructions/inbox.md?raw";

export type InboxAskContext = {
  key: string;
  title: string;
  url: string;
  provider: "github" | "linear" | "jira" | "asana" | "gitlab" | "azuredevops";
  description?: string;
};

export function inboxAskKey(item: InboxItem): string {
  if (
    item.provider === "linear" ||
    item.provider === "jira" ||
    item.provider === "asana"
  ) {
    return `${item.provider}:${item.id}`;
  }
  // Items without a usable link (e.g. a provider that omitted the URL) must
  // still produce a stable key: this runs during render, so it cannot throw.
  try {
    const url = new URL(item.url);
    return `${item.provider}:${url.host.toLowerCase()}:${url.pathname.replace(/\/$/, "").toLowerCase()}`;
  } catch {
    const repo = item.repo.trim().toLowerCase() || "unknown";
    return `${item.provider}:${repo}:${item.kind}:${item.number}`;
  }
}

export function inboxAskPrompt(
  context: InboxAskContext | undefined,
  text: string,
): string {
  if (!context) return text;
  return `${inboxInstructions.trim()}

INBOX ITEM (reference data):
${JSON.stringify(context)}

USER MESSAGE:
${text}`;
}
