import { sonarLabel } from "./sonarLabels";
import { recordInboxSelfActivity } from "./inboxSelfActivity";
import { sonarHtmlToMarkdown } from "./sonarRuleContent";
import { invoke } from "@tauri-apps/api/core";
import type { InboxItem, GithubWorkItemDetails } from "./githubTasks";

export type SonarProject = { key: string; name: string };
export type SonarOrganization = { key: string; name: string };
export type SonarStatus = {
  connected: boolean;
  site: string;
  /** Empty on SonarQube Server, which has no organizations. */
  organization: string;
  projects: SonarProject[];
  /** The one project the inbox reads. Empty until a project is watched. */
  selectedProject: string;
};
export type SonarBranch = {
  projectKey: string;
  repositoryName?: string;
  name: string;
  isMain: boolean;
  analysisDate: string;
};
export type SonarIssue = {
  key: string;
  repositoryName?: string;
  projectKey: string;
  branch: string;
  message: string;
  severity: string;
  rule: string;
  issueType: string;
  status: string;
  resolved: boolean;
  file: string;
  line: number | null;
  updatedAt: string;
  assignee: string;
  url: string;
};
export type SonarSnapshot = {
  issues: SonarIssue[];
  branches: SonarBranch[];
  errors: string[];
};
export const SONAR_CHANGE_EVENT = "monocode:sonar-change";
// Remembered so a remounted inbox can draw the project row before the status read returns.
let lastSonarStatus: SonarStatus | null = null;
const rememberSonarStatus = (status: SonarStatus) => (lastSonarStatus = status);
export const peekSonarStatus = () => lastSonarStatus;
export const sonarConnected = () =>
  invoke<SonarStatus>("sonar_status").then(rememberSonarStatus);
async function updateSonar(
  command: string,
  args?: Record<string, unknown>,
): Promise<SonarStatus> {
  const status = rememberSonarStatus(await invoke<SonarStatus>(command, args));
  clearSonarContent();
  window.dispatchEvent(new Event(SONAR_CHANGE_EVENT));
  return status;
}
export const connectSonar = (config: { site: string; token: string }) =>
  updateSonar("sonar_connect", config);
export const disconnectSonar = () => updateSonar("sonar_disconnect");
export const listSonarOrganizations = () =>
  invoke<SonarOrganization[]>("sonar_list_organizations");
export const listSonarProjects = () =>
  invoke<SonarProject[]>("sonar_list_projects");
export const setSonarOrganization = (organization: string) =>
  updateSonar("sonar_set_organization", { organization });
export const setSonarProjects = (projects: SonarProject[]) =>
  updateSonar("sonar_set_projects", { projects });
export const selectSonarProject = (projectKey: string) =>
  updateSonar("sonar_select_project", { projectKey });
export const listSonarIssues = (query: {
  assignedToMe: boolean;
  state: "open" | "all";
}) =>
  invoke<SonarSnapshot>("sonar_list_issues", {
    assignedToMe: query.assignedToMe,
    state: query.state,
  });

// Preserve case and scope branch names to their project. JSON avoids delimiter collisions.
export function sonarBranchKey(projectKey: string, branch: string): string {
  return JSON.stringify([projectKey, branch]);
}

export function sonarIssueToInboxItem(issue: SonarIssue): InboxItem {
  return {
    provider: "sonar",
    kind: "sonar",
    id: issue.key,
    identifier: issue.rule || issue.key,
    number: 0,
    title: issue.message,
    url: issue.url,
    state: issue.resolved ? "closed" : "open",
    stateType: issue.resolved ? "done" : "new",
    updatedAt: issue.updatedAt,
    labels: [issue.severity, issue.issueType]
      .filter(Boolean)
      .map((name) => ({ name: sonarLabel(name), color: "" })),
    assignees: issue.assignee ? [{ login: issue.assignee }] : [],
    draft: false,
    repo: issue.projectKey,
    projectId: issue.projectKey,
    projectName: issue.repositoryName || issue.projectKey,
    projectPath: "",
    branch: issue.branch,
    sonarIssue: {
      severity: issue.severity,
      rule: issue.rule,
      issueType: issue.issueType,
      status: issue.status,
      file: issue.file,
      line: issue.line,
      assignee: issue.assignee,
    },
  };
}

export function sonarIssueDetails(
  item: InboxItem,
  content = peekSonarIssueContent(item),
): GithubWorkItemDetails {
  const issue = item.sonarIssue;
  const body = issue
    ? [
        item.title,
        "",
        `Project: ${item.repo}`,
        `Branch: ${item.branch}`,
        `Rule: ${issue.rule}`,
        `Severity: ${issue.severity}`,
        `Status: ${issue.status}`,
        `File: ${issue.file}${issue.line == null ? "" : `:${issue.line}`}`,
      ].join("\n")
    : item.title;
  const extra = content
    ? [
        ...(content.source.length
          ? [
              "",
              "Analyzed source:",
              "```",
              ...content.source.map((line) => `${line.line}: ${line.code}`),
              "```",
            ]
          : []),
        ...sonarRuleGuidance(content).flatMap((section) => [
          "",
          section.title,
          section.markdown,
        ]),
        ...(content.referenceDocumentation
          ? [
              "",
              "Guidance is from the latest official Sonar analyzer documentation.",
            ]
          : []),
        "",
        content.ruleUrl,
      ].join("\n")
    : "";
  return { body: body + extra, author: "" };
}

export type SonarIssueContent = {
  repositoryName: string;
  comparison?: { baseBranch: string; original: string; current: string } | null;
  issue: {
    textRange?: {
      startLine: number;
      endLine: number;
      startOffset?: number;
      endOffset?: number;
    };
    line?: number;
    status?: string;
    issueStatus?: string;
    transitions?: string[];
    effort?: string;
    debt?: string;
    creationDate?: string;
    updateDate?: string;
    tags?: string[];
    impacts?: { softwareQuality: string; severity: string }[];
    flows?: {
      locations: {
        component?: string;
        msg?: string;
        textRange?: { startLine: number; endLine: number };
      }[];
    }[];
    comments?: {
      key: string;
      login?: string;
      htmlText?: string;
      markdown?: string;
      createdAt?: string;
    }[];
  };
  rule: {
    name?: string;
    lang?: string;
    htmlDesc?: string;
    mdDesc?: string;
    descriptionSections?: {
      key: string;
      content: string;
      context?: { displayName?: string; key?: string };
    }[];
    params?: { key: string; htmlDesc?: string; defaultValue?: string }[];
    cleanCodeAttribute?: string;
  } | null;
  ruleHtml: string;
  ruleUrl: string;
  referenceDocumentation: boolean;
  source: { line: number; code: string }[];
  errors: string[];
};
const contents = new Map<string, SonarIssueContent>();
const contentInflight = new Map<string, Promise<SonarIssueContent>>();
let contentGeneration = 0;
function contentKey(item: InboxItem) {
  return JSON.stringify([item.repo, item.branch, item.id, item.updatedAt]);
}
export function clearSonarContent() {
  contentGeneration += 1;
  contents.clear();
  contentInflight.clear();
}
export function peekSonarIssueContent(
  item: InboxItem,
): SonarIssueContent | undefined {
  return contents.get(contentKey(item));
}
export function loadSonarIssueContent(
  item: InboxItem,
  options?: { force?: boolean },
): Promise<SonarIssueContent> {
  const key = contentKey(item);
  const cached = contents.get(key);
  if (cached && !options?.force) return Promise.resolve(cached);
  const pending = contentInflight.get(key);
  if (pending) return pending;
  const generation = contentGeneration;
  const promise = invoke<SonarIssueContent>("sonar_issue_content", {
    projectKey: item.repo,
    branch: item.branch ?? "",
    issueKey: item.id ?? "",
  })
    .then((content) => {
      if (generation === contentGeneration) {
        if (contents.size >= 50) contents.delete(contents.keys().next().value!);
        contents.set(key, content);
      }
      return content;
    })
    .finally(() => {
      if (contentInflight.get(key) === promise) contentInflight.delete(key);
    });
  contentInflight.set(key, promise);
  return promise;
}

export function sonarRuleGuidance(
  content: SonarIssueContent,
): { title: string; markdown: string; html: string }[] {
  const sections =
    content.rule?.descriptionSections?.filter((section) =>
      section.content.trim(),
    ) ?? [];
  if (sections.length && !content.referenceDocumentation)
    return sections.map((section) => {
      const title =
        (
          {
            root_cause: "Why is this an issue?",
            how_to_fix: "How to fix it",
            resources: "Resources",
            introduction: "Overview",
          } as Record<string, string>
        )[section.key] ?? section.key.replace(/_/g, " ");
      return {
        title: section.context?.displayName
          ? `${title} · ${section.context.displayName}`
          : title,
        markdown: sonarHtmlToMarkdown(section.content),
        html: section.content,
      };
    });
  const html = content.ruleHtml || content.rule?.htmlDesc || "";
  return html
    ? [{ title: "Rule guidance", markdown: sonarHtmlToMarkdown(html), html }]
    : content.rule?.mdDesc
      ? [{ title: "Rule guidance", markdown: content.rule.mdDesc, html: "" }]
      : [];
}

export async function transitionSonarIssue(
  item: InboxItem,
  transition: string,
  comment: string,
  shareFeedback: boolean,
): Promise<InboxItem> {
  const updated = await invoke<SonarIssue>("sonar_transition_issue", {
    request: {
      projectKey: item.repo,
      branch: item.branch ?? "",
      issueKey: item.id ?? "",
      transition,
      comment: comment.trim(),
      shareFeedback,
    },
  });
  const repositoryName =
    updated.repositoryName && updated.repositoryName !== updated.projectKey
      ? updated.repositoryName
      : item.projectName;
  const next = {
    ...item,
    ...sonarIssueToInboxItem({ ...updated, repositoryName }),
  };
  clearSonarContent();
  recordInboxSelfActivity({
    provider: "sonar",
    kind: "sonar",
    id: item.id,
    repo: item.repo,
    branch: item.branch,
  });
  return next;
}
