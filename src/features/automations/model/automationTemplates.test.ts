import { describe, expect, it } from "vitest";
import {
  matchInboxAutomations,
  SUPPORTED_INBOX_TRIGGER_EVENTS,
} from "./automationEvents";
import { draftFromTemplate, type Automation } from "./automations";
import type { InboxItem } from "../../inbox/model/githubTasks";
import {
  AUTOMATION_TEMPLATE_CATEGORIES,
  AUTOMATION_TEMPLATES,
  templatesForCategory,
} from "./automationTemplates";

describe("automation templates", () => {
  it("keeps popular examples in their real category too", () => {
    const popular = templatesForCategory("popular");
    expect(popular.map((template) => template.id)).toEqual([
      "find-critical-bugs",
      "scan-vulnerabilities",
      "generate-docs",
      "add-test-coverage",
    ]);
    expect(templatesForCategory("review").some((t) => t.popular)).toBe(true);
    expect(templatesForCategory("security").some((t) => t.popular)).toBe(true);
  });

  it("covers every gallery category with at least one example", () => {
    for (const category of AUTOMATION_TEMPLATE_CATEGORIES) {
      expect(templatesForCategory(category.id).length).toBeGreaterThan(0);
    }
    expect(AUTOMATION_TEMPLATES.every((template) => template.prompt.trim())).toBe(
      true,
    );
  });

  it("only uses inbox events that actually fire", () => {
    for (const template of AUTOMATION_TEMPLATES) {
      if (template.trigger.kind === "time") continue;
      const supported =
        SUPPORTED_INBOX_TRIGGER_EVENTS[
          template.trigger.kind as keyof typeof SUPPORTED_INBOX_TRIGGER_EVENTS
        ];
      expect(supported).toContain(template.trigger.event);
    }
  });

  it("gives every template a unique id", () => {
    const ids = AUTOMATION_TEMPLATES.map((template) => template.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("auto-fix GitHub issues template", () => {
  const template = AUTOMATION_TEMPLATES.find(
    (candidate) => candidate.id === "autofix-github-issues",
  );

  function savedFromTemplate(): Automation {
    if (!template) throw new Error("autofix-github-issues template is missing");
    return {
      ...draftFromTemplate("/tmp/web", "claude", "model", template),
      id: "autofix",
      nextRunAt: 2,
      createdAt: 1,
      updatedAt: 1,
    };
  }

  function inboxItem(overrides: Partial<InboxItem> = {}): InboxItem {
    return {
      provider: "github",
      kind: "issue",
      number: 4821,
      title: "Trace filter drops unnamed traces",
      url: "https://github.com/acme/web/issues/4821",
      state: "open",
      updatedAt: "2026-09-30T15:00:00Z",
      createdAt: "2026-09-30T15:00:00Z",
      labels: [],
      assignees: [],
      draft: false,
      repo: "acme/web",
      projectPath: "/tmp/web",
      ...overrides,
    };
  }

  it("is offered next to the triage example", () => {
    expect(
      templatesForCategory("incidents").map((candidate) => candidate.id),
    ).toContain("autofix-github-issues");
  });

  it("starts unattended in its own worktree so a run cannot touch the open checkout", () => {
    const saved = savedFromTemplate();
    expect(saved.workspaceMode).toBe("worktree");
    expect(saved.runtimeMode).toBe("auto");
    expect(saved.reuseSession).toBe(false);
    expect(saved.enabled).toBe(true);
  });

  it("fires for a new issue in its project and hands the agent the issue number and URL", () => {
    const matches = matchInboxAutomations([savedFromTemplate()], [inboxItem()]);
    expect(matches).toHaveLength(1);
    expect(matches[0].prompt).toContain("#4821 Trace filter drops unnamed traces");
    expect(matches[0].prompt).toContain(
      "https://github.com/acme/web/issues/4821",
    );
  });

  it("ignores pull requests, other projects, and other providers", () => {
    const saved = savedFromTemplate();
    const ignored = [
      inboxItem({ kind: "pr", url: "https://github.com/acme/web/pull/4821" }),
      inboxItem({ kind: "pr", draft: true }),
      inboxItem({ projectPath: "/tmp/other" }),
      inboxItem({ provider: "gitlab" }),
    ];
    expect(matchInboxAutomations([saved], ignored)).toEqual([]);
  });

  it("names the branch after the issue and never reuses an attempted one", () => {
    const prompt = template?.prompt ?? "";
    expect(prompt).toContain("git branch -m autofix/issue-<n>");
    expect(prompt).toContain("git ls-remote --heads origin autofix/issue-<n>");
    expect(prompt).toContain("git push -u origin autofix/issue-<n>");
    expect(prompt).not.toMatch(/checkout -[bB] autofix/);
  });

  it("keeps the guardrails for running on publicly written issues", () => {
    const prompt = template?.prompt ?? "";
    expect(prompt).toMatch(/untrusted/i);
    expect(prompt).toContain("Never open a PR");
    expect(prompt).toMatch(/Never comment on/);
    expect(prompt).toMatch(/never force-push/i);
    expect(prompt).not.toMatch(/gh pr create|gh issue comment/);
  });

  it("works for any repository instead of one hard-coded project", () => {
    const prompt = template?.prompt ?? "";
    expect(prompt).not.toMatch(/langfuse|0xvasanth/i);
    expect(prompt).toContain("--repo <repo>");
  });
});
