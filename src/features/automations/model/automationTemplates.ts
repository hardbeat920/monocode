import { t } from "../../../shared/i18n/locale";
import {
  automationWeekdayLabel,
  type AutomationScheduleKind,
  type AutomationTriggerKind,
} from "./automations";

export const AUTOMATION_TEMPLATE_CATEGORIES = [
  {
    id: "popular",
    get label() {
      return t("automations.category.popular");
    },
  },
  {
    id: "review",
    get label() {
      return t("automations.category.review");
    },
  },
  {
    id: "security",
    get label() {
      return t("automations.category.security");
    },
  },
  {
    id: "incidents",
    get label() {
      return t("automations.category.incidents");
    },
  },
  {
    id: "research",
    get label() {
      return t("automations.category.research");
    },
  },
  {
    id: "environment",
    get label() {
      return t("automations.category.environment");
    },
  },
] as const;

export type AutomationTemplateCategoryId =
  (typeof AUTOMATION_TEMPLATE_CATEGORIES)[number]["id"];

export type AutomationTemplateIcon =
  | "search"
  | "alert"
  | "file"
  | "check"
  | "lock"
  | "pr"
  | "inbox"
  | "gauge"
  | "terminal"
  | "note";

export type AutomationTemplate = {
  id: string;
  category: Exclude<AutomationTemplateCategoryId, "popular">;
  popular?: boolean;
  icon: AutomationTemplateIcon;
  name: string;
  description: string;
  prompt: string;
  trigger: {
    kind: AutomationTriggerKind;
    event: string;
    scheduleKind?: AutomationScheduleKind;
    time?: string;
    dayOfWeek?: number;
    minute?: number;
  };
  triggerLabel: string;
};

const weekdaysAt = (time: string) =>
  t("automations.schedule.weekdaysAt", { time });
const weeklyAt = (dayOfWeek: number, time: string) =>
  t("automations.schedule.weeklyAt", {
    day: automationWeekdayLabel(dayOfWeek) ?? "",
    time,
  });

/**
 * Text fields are getters so they follow the current language each time a
 * template is read. The prompts are sent to the model and stay in English.
 */
export const AUTOMATION_TEMPLATES: AutomationTemplate[] = [
  {
    id: "find-critical-bugs",
    category: "review",
    popular: true,
    icon: "alert",
    get name() {
      return t("automations.template.findCriticalBugs.name");
    },
    get description() {
      return t("automations.template.findCriticalBugs.description");
    },
    trigger: {
      kind: "time",
      event: "weekdays",
      scheduleKind: "weekdays",
      time: "09:00",
    },
    get triggerLabel() {
      return weekdaysAt("09:00");
    },
    prompt: `Review recent git history in this repo for high-severity correctness bugs.

Focus on:
- Logic errors, race conditions, and data loss
- Broken error handling that can fail silently in production
- Regressions introduced in the last few days of commits

Only report issues you can validate from the current code. If a fix is clearly safe and local, implement it. Skip style nits and speculative issues.

At the end, summarize what you found, what you changed, and anything that still needs a human.`,
  },
  {
    id: "scan-vulnerabilities",
    category: "security",
    popular: true,
    icon: "search",
    get name() {
      return t("automations.template.scanVulnerabilities.name");
    },
    get description() {
      return t("automations.template.scanVulnerabilities.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "10:00",
      dayOfWeek: 1,
    },
    get triggerLabel() {
      return weeklyAt(1, "10:00");
    },
    prompt: `Perform a security review of this repository.

Look for:
- Injection, XSS, SSRF, and auth/authz bypasses
- Secrets, tokens, or credentials committed to the repo
- Unsafe deserialization, path traversal, and command injection
- Dependency or config issues that meaningfully increase risk

Only report issues you can validate with concrete evidence. Do not invent CVEs. Rank findings by impact and include the file path, why it is exploitable, and a recommended fix. Implement safe, local remediations when the change is clearly correct.`,
  },
  {
    id: "generate-docs",
    category: "research",
    popular: true,
    icon: "file",
    get name() {
      return t("automations.template.generateDocs.name");
    },
    get description() {
      return t("automations.template.generateDocs.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "09:00",
      dayOfWeek: 1,
    },
    get triggerLabel() {
      return weeklyAt(1, "09:00");
    },
    prompt: `Update developer documentation for this repo based on recent changes.

- Find APIs, modules, and workflows that are new, renamed, or under-documented
- Prefer editing existing docs over creating new files
- Keep the writing concise and accurate; do not invent behavior
- Include setup, how to run, and the main entry points if those are missing

Open a concise summary of what docs you changed and why.`,
  },
  {
    id: "add-test-coverage",
    category: "review",
    popular: true,
    icon: "check",
    get name() {
      return t("automations.template.addTestCoverage.name");
    },
    get description() {
      return t("automations.template.addTestCoverage.description");
    },
    trigger: {
      kind: "time",
      event: "weekdays",
      scheduleKind: "weekdays",
      time: "11:00",
    },
    get triggerLabel() {
      return weekdaysAt("11:00");
    },
    prompt: `Look at recent commits and add tests for high-risk logic that is missing coverage.

- Prefer the project's existing test runner and style
- Target correctness, edge cases, and regressions — not coverage for its own sake
- Do not rewrite production code unless a test reveals a clear bug
- Run the relevant tests and fix anything you break

Summarize which tests you added and which gaps remain.`,
  },
  {
    id: "review-pull-requests",
    category: "review",
    icon: "pr",
    get name() {
      return t("automations.template.reviewPullRequests.name");
    },
    get description() {
      return t("automations.template.reviewPullRequests.description");
    },
    trigger: {
      kind: "github",
      event: "pull_request_opened",
    },
    get triggerLabel() {
      return t("automations.trigger.pullRequestOpened");
    },
    prompt: `Review the newly opened pull request.

Check for:
- Correctness bugs and regressions
- Missing tests for the changed behavior
- Security or data-loss risks
- API / contract breakage

Leave a structured review: blockers first, then suggestions. Do not nitpick formatting. If the change is good, say so briefly and note residual risk.`,
  },
  {
    id: "review-draft-prs",
    category: "review",
    icon: "pr",
    get name() {
      return t("automations.template.reviewDraftPrs.name");
    },
    get description() {
      return t("automations.template.reviewDraftPrs.description");
    },
    trigger: {
      kind: "github",
      event: "draft_opened",
    },
    get triggerLabel() {
      return t("automations.trigger.draftOpened");
    },
    prompt: `A draft pull request was opened. Give early, high-signal feedback.

Focus on direction, missing tests, and likely bugs — not polish. Call out anything that will be expensive to change later. Keep the review short and specific to the diff.`,
  },
  {
    id: "dependency-audit",
    category: "security",
    icon: "lock",
    get name() {
      return t("automations.template.dependencyAudit.name");
    },
    get description() {
      return t("automations.template.dependencyAudit.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "09:30",
      dayOfWeek: 1,
    },
    get triggerLabel() {
      return weeklyAt(1, "09:30");
    },
    prompt: `Audit this repo's dependencies.

- Inspect lockfiles and package manifests for vulnerable, unused, or unexpectedly upgraded packages
- Confirm findings against the project's current tooling (npm, cargo, etc.)
- Only propose upgrades or removals you can justify
- Do not bump majors unless the current version is unsafe and the upgrade is clearly required

Report what is risky, what you changed, and what still needs a human.`,
  },
  {
    id: "secret-scan",
    category: "security",
    icon: "lock",
    get name() {
      return t("automations.template.secretScan.name");
    },
    get description() {
      return t("automations.template.secretScan.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "09:30",
      dayOfWeek: 1,
    },
    get triggerLabel() {
      return weeklyAt(1, "09:30");
    },
    prompt: `Scan the working tree and recent git history for secrets.

Look for API keys, tokens, private keys, .env files, and credentials in config. If you find a real secret, do not echo the full value. Report the file and a redacted snippet, explain why it is sensitive, and recommend rotation plus a git-history cleanup if it was committed.`,
  },
  {
    id: "triage-github-issues",
    category: "incidents",
    icon: "inbox",
    get name() {
      return t("automations.template.triageGithubIssues.name");
    },
    get description() {
      return t("automations.template.triageGithubIssues.description");
    },
    trigger: {
      kind: "github",
      event: "issue_opened",
    },
    get triggerLabel() {
      return t("automations.trigger.issueOpened");
    },
    prompt: `A new GitHub issue was opened. Triage it against this repo.

- Reproduce or locate the relevant code if the report is specific enough
- Label the severity in your summary (blocker / bug / request / unclear)
- Add a concrete next step: file paths, likely cause, or the missing information
- Do not implement a large fix unless the issue is clearly a small, validated bug`,
  },
  {
    id: "triage-new-issues",
    category: "incidents",
    icon: "inbox",
    get name() {
      return t("automations.template.triageNewIssues.name");
    },
    get description() {
      return t("automations.template.triageNewIssues.description");
    },
    trigger: {
      kind: "linear",
      event: "issue_created",
    },
    get triggerLabel() {
      return t("automations.trigger.issueCreated");
    },
    prompt: `A new Linear issue was created. Triage it against this repo.

- Reproduce or locate the relevant code if the report is specific enough
- Label the severity in your summary (blocker / bug / request / unclear)
- Add a concrete next step: file paths, likely cause, or the missing information
- Do not implement a large fix unless the issue is clearly a small, validated bug`,
  },
  {
    id: "failing-ci-watch",
    category: "incidents",
    icon: "alert",
    get name() {
      return t("automations.template.failingCiWatch.name");
    },
    get description() {
      return t("automations.template.failingCiWatch.description");
    },
    trigger: {
      kind: "time",
      event: "weekdays",
      scheduleKind: "weekdays",
      time: "08:30",
    },
    get triggerLabel() {
      return weekdaysAt("08:30");
    },
    prompt: `Run the project's existing test / lint / typecheck commands.

If something fails:
- Identify the first real failure, not the cascade
- Fix it if the cause is local and obvious
- Otherwise write a short diagnosis with the command, the error, and the suspected file

Do not add new test infrastructure. Do not "fix" flakes by weakening assertions.`,
  },
  {
    id: "weekly-changelog",
    category: "research",
    icon: "note",
    get name() {
      return t("automations.template.weeklyChangelog.name");
    },
    get description() {
      return t("automations.template.weeklyChangelog.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "16:00",
      dayOfWeek: 5,
    },
    get triggerLabel() {
      return weeklyAt(5, "16:00");
    },
    prompt: `Write a concise changelog for this repo covering the last 7 days of commits.

Group by user-facing changes, fixes, and internal work. Skip noise (formatting, lockfile-only, merge commits). Use the project's existing changelog or docs style if one exists; otherwise write a short markdown summary. Do not invent features that are not in the commits.`,
  },
  {
    id: "repo-health",
    category: "environment",
    icon: "gauge",
    get name() {
      return t("automations.template.repoHealth.name");
    },
    get description() {
      return t("automations.template.repoHealth.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "09:00",
      dayOfWeek: 1,
    },
    get triggerLabel() {
      return weeklyAt(1, "09:00");
    },
    prompt: `Do a repo health check.

- Working tree cleanliness and leftover build artifacts that should be gitignored
- README / setup instructions that no longer match the project
- Obvious CI, lint, or typecheck config drift
- Stale or broken scripts in package.json / Makefile / justfile

Fix the small, clearly correct issues. Report the rest with file paths. Do not do a broad refactor.`,
  },
  {
    id: "install-doctor",
    category: "environment",
    icon: "terminal",
    get name() {
      return t("automations.template.installDoctor.name");
    },
    get description() {
      return t("automations.template.installDoctor.description");
    },
    trigger: {
      kind: "time",
      event: "weekly",
      scheduleKind: "weekly",
      time: "10:00",
      dayOfWeek: 1,
    },
    get triggerLabel() {
      return weeklyAt(1, "10:00");
    },
    prompt: `Verify this project still sets up cleanly.

Follow the README / documented install steps as closely as possible. Note any missing prerequisites, broken scripts, or docs that don't match reality. Fix small doc or script issues. Do not change application architecture.

End with a pass/fail and the exact commands you ran.`,
  },
];

export function templatesForCategory(
  category: AutomationTemplateCategoryId,
): AutomationTemplate[] {
  if (category === "popular") {
    return AUTOMATION_TEMPLATES.filter((template) => template.popular);
  }
  return AUTOMATION_TEMPLATES.filter(
    (template) => template.category === category,
  );
}
