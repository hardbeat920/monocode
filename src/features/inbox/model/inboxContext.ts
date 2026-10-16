import { sonarIssueDetails, loadSonarIssueContent } from "./sonar";
import type { InboxItem } from "./githubTasks";
import { isTrackerSource } from "./inboxProviders";
import { jiraIssueDetails, peekJiraIssueDetails } from "./jira";
import { linearIssueDetails, peekLinearIssueDetails } from "./linear";

/** Load tracker context before opening a session, including from list actions. */
export async function inboxTrackerDescription(
  item: InboxItem,
  body?: string,
): Promise<string | undefined> {
  if (item.provider === "sonar") {
    if (body !== undefined) return body;
    const content = await loadSonarIssueContent(item).catch(() => undefined);
    return sonarIssueDetails(item, content).body;
  }
  if (!isTrackerSource(item.provider)) return body;
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
