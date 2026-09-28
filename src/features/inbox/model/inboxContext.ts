import type { InboxItem } from "./githubTasks";
import { jiraIssueDetails, peekJiraIssueDetails } from "./jira";
import { linearIssueDetails, peekLinearIssueDetails } from "./linear";

/** Load tracker context before opening a session, including from list actions. */
export async function inboxTrackerDescription(
  item: InboxItem,
  body?: string,
): Promise<string | undefined> {
  if (item.provider !== "linear" && item.provider !== "jira") return body;
  if (body !== undefined) return body;
  if (item.provider === "jira") {
    const key = item.identifier?.trim();
    if (!key) throw new Error("Missing Jira issue key");
    return (peekJiraIssueDetails(key) ?? (await jiraIssueDetails(key))).body;
  }
  if (!item.id) throw new Error("Missing Linear issue");
  return (
    peekLinearIssueDetails(item.id) ?? (await linearIssueDetails(item.id))
  ).body;
}
