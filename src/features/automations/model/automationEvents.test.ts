// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import {
  automationEventKey,
  claimInboxAutomationRuns,
  inboxAppearedEvent,
  matchInboxAutomations,
} from "./automationEvents";
import {
  createAutomationTrigger,
  newAutomationDraft,
  type Automation,
} from "./automations";
import type { InboxItem } from "../../inbox/model/githubTasks";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const at = (value: string) => new Date(value).getTime();

function item(overrides: Partial<InboxItem> = {}): InboxItem {
  return {
    provider: "github",
    kind: "pr",
    number: 12,
    title: "Fix checkout",
    url: "https://github.com/acme/web/pull/12",
    state: "open",
    updatedAt: "2026-09-19T15:00:00Z",
    createdAt: "2026-09-19T15:00:00Z",
    labels: [],
    assignees: [],
    draft: false,
    repo: "acme/web",
    projectPath: "/tmp/web",
    ...overrides,
  };
}

function automation(
  overrides: Partial<Automation> & { triggers: Automation["triggers"] },
): Automation {
  const draft = newAutomationDraft("/tmp/web", "codex", "model");
  return {
    ...draft,
    id: "automation-id",
    name: "Review pull requests",
    prompt: "Review the newly opened pull request.",
    nextRunAt: at("2026-09-21T09:00:00"),
    createdAt: at("2026-09-19T09:00:00"),
    updatedAt: at("2026-09-19T10:00:00"),
    ...overrides,
  };
}

describe("inbox automation events", () => {
  it("matches account-wide Jira issues and keeps their event identity after a project move", () => {
    const jira = item({ provider: "jira", kind: "jira", id: "10042", identifier: "ENG-42", repo: "ENG", projectPath: "" });
    const trigger = { ...createAutomationTrigger("jira", "issue_created"), repos: ["ENG"] };
    expect(inboxAppearedEvent(jira)).toEqual({ kind: "jira", event: "issue_created" });
    expect(automationEventKey(jira)).toBe("jira:issue:10042");
    expect(automationEventKey({ ...jira, identifier: "OPS-17", repo: "OPS", number: 17 })).toBe("jira:issue:10042");
    const matches = matchInboxAutomations([automation({ triggers: [trigger] })], [jira]);
    expect(matches).toHaveLength(1);
    expect(matches[0].prompt).toContain("Work on this Jira issue:");
    expect(matchInboxAutomations([automation({ triggers: [trigger] })], [{ ...jira, repo: "OPS" }])).toEqual([]);
  });

  it("maps opened PRs, drafts, and issues", () => {
    expect(inboxAppearedEvent(item())).toEqual({
      kind: "github",
      event: "pull_request_opened",
    });
    expect(inboxAppearedEvent(item({ draft: true }))).toEqual({
      kind: "github",
      event: "draft_opened",
    });
    expect(
      inboxAppearedEvent(
        item({ kind: "issue", url: "https://github.com/acme/web/issues/12" }),
      ),
    ).toEqual({ kind: "github", event: "issue_opened" });
    expect(
      inboxAppearedEvent(
        item({
          provider: "gitlab",
          url: "https://gitlab.example.com/acme/web/-/merge_requests/12",
        }),
      ),
    ).toEqual({ kind: "gitlab", event: "merge_request_opened" });
    expect(
      inboxAppearedEvent(
        item({
          provider: "gitlab",
          kind: "issue",
          url: "https://gitlab.example.com/acme/web/-/issues/12",
        }),
      ),
    ).toEqual({ kind: "gitlab", event: "issue_opened" });
    expect(
      inboxAppearedEvent(
        item({
          provider: "linear",
          kind: "linear",
          id: "issue-1",
          identifier: "ENG-12",
          url: "https://linear.app/acme/issue/ENG-12",
          projectPath: "",
        }),
      ),
    ).toEqual({ kind: "linear", event: "issue_created" });
    expect(
      inboxAppearedEvent(
        item({
          provider: "azuredevops",
          url: "https://dev.azure.com/acme/shop/_git/web/pullrequest/12",
        }),
      ),
    ).toEqual({ kind: "azuredevops", event: "pull_request_appeared" });
    expect(
      inboxAppearedEvent(
        item({
          provider: "azuredevops",
          kind: "issue",
          url: "https://dev.azure.com/acme/shop/_workitems/edit/12",
        }),
      ),
    ).toEqual({ kind: "azuredevops", event: "work_item_appeared" });
  });

  it("fires a same-project opened PR into the matching automation", () => {
    const review = automation({
      triggers: [createAutomationTrigger("github", "pull_request_opened")],
    });
    const [match] = matchInboxAutomations([review], [item()]);
    expect(match?.automation.id).toBe("automation-id");
    expect(match?.eventKey).toBe("github:pr:acme/web:12");
    expect(match?.occurredAt).toBe(at("2026-09-19T15:00:00Z"));
    expect(match?.prompt).toContain("Review the newly opened pull request.");
    expect(match?.prompt).toContain("Work on this GitHub pull request:");
    expect(match?.prompt).toContain("https://github.com/acme/web/pull/12");
  });

  it("does not treat a draft as a ready pull request", () => {
    const review = automation({
      triggers: [createAutomationTrigger("github", "pull_request_opened")],
    });
    expect(
      matchInboxAutomations([review], [item({ draft: true })]),
    ).toEqual([]);
    const drafts = automation({
      id: "draft-id",
      triggers: [createAutomationTrigger("github", "draft_opened")],
    });
    expect(matchInboxAutomations([drafts], [item({ draft: true })])).toHaveLength(
      1,
    );
  });

  it("keeps automations scoped to their project", () => {
    const review = automation({
      cwd: "/tmp/other",
      triggers: [createAutomationTrigger("github", "pull_request_opened")],
    });
    expect(matchInboxAutomations([review], [item()])).toEqual([]);
  });

  it("fires a same-project opened GitHub issue into the matching automation", () => {
    const triage = automation({
      name: "Triage GitHub issues",
      prompt: "Triage the newly opened GitHub issue.",
      triggers: [createAutomationTrigger("github", "issue_opened")],
    });
    const [match] = matchInboxAutomations(
      [triage],
      [item({ kind: "issue", url: "https://github.com/acme/web/issues/12" })],
    );
    expect(match?.eventKey).toBe("github:issue:acme/web:12");
    expect(match?.prompt).toContain("Triage the newly opened GitHub issue.");
    expect(match?.prompt).toContain("Work on this GitHub issue:");
    expect(
      matchInboxAutomations([triage], [item()]),
    ).toEqual([]);
  });

  it("fires GitLab merge requests into the matching project", () => {
    const review = automation({
      triggers: [createAutomationTrigger("gitlab", "merge_request_opened")],
    });
    const [match] = matchInboxAutomations(
      [review],
      [
        item({
          provider: "gitlab",
          url: "https://gitlab.example.com/acme/web/-/merge_requests/12",
        }),
      ],
    );
    expect(match?.eventKey).toBe("gitlab:pr:acme/web:12");
    expect(match?.prompt).toContain("Work on this GitLab merge request:");
  });

  it("fires GitLab issues separately from merge requests", () => {
    const triage = automation({
      triggers: [createAutomationTrigger("gitlab", "issue_opened")],
    });
    expect(
      matchInboxAutomations(
        [triage],
        [
          item({
            provider: "gitlab",
            url: "https://gitlab.example.com/acme/web/-/merge_requests/12",
          }),
        ],
      ),
    ).toEqual([]);
    const [match] = matchInboxAutomations(
      [triage],
      [
        item({
          provider: "gitlab",
          kind: "issue",
          url: "https://gitlab.example.com/acme/web/-/issues/12",
        }),
      ],
    );
    expect(match?.eventKey).toBe("gitlab:issue:acme/web:12");
    expect(match?.prompt).toContain("Work on this GitLab issue:");
  });

  it("fires Azure DevOps pull requests into the matching project", () => {
    const review = automation({
      triggers: [
        createAutomationTrigger("azuredevops", "pull_request_appeared"),
      ],
    });
    const [match] = matchInboxAutomations(
      [review],
      [
        item({
          provider: "azuredevops",
          url: "https://dev.azure.com/acme/shop/_git/web/pullrequest/12",
        }),
      ],
    );
    expect(match?.eventKey).toBe("azuredevops:pr:acme/web:12");
    expect(match?.prompt).toContain("Work on this ADO pull request:");
  });

  it("fires Azure DevOps work items separately from pull requests", () => {
    const triage = automation({
      triggers: [createAutomationTrigger("azuredevops", "work_item_appeared")],
    });
    expect(
      matchInboxAutomations(
        [triage],
        [
          item({
            provider: "azuredevops",
            url: "https://dev.azure.com/acme/shop/_git/web/pullrequest/12",
          }),
        ],
      ),
    ).toEqual([]);
    const [match] = matchInboxAutomations(
      [triage],
      [
        item({
          provider: "azuredevops",
          kind: "issue",
          url: "https://dev.azure.com/acme/shop/_workitems/edit/12",
        }),
      ],
    );
    expect(match?.eventKey).toBe("azuredevops:issue:acme/web:12");
    expect(match?.prompt).toContain("Work on this ADO issue:");
  });

  it("fires new Linear issues into the automation's project", () => {
    const triage = automation({
      name: "Triage new issues",
      prompt: "Triage the new Linear issue.",
      triggers: [createAutomationTrigger("linear", "issue_created")],
    });
    const [match] = matchInboxAutomations(
      [triage],
      [
        item({
          provider: "linear",
          kind: "linear",
          id: "issue-1",
          identifier: "ENG-12",
          title: "Fix auth",
          url: "https://linear.app/acme/issue/ENG-12",
          projectPath: "",
        }),
      ],
    );
    expect(match?.eventKey).toBe("linear:issue:issue-1");
    expect(match?.automation.cwd).toBe("/tmp/web");
    expect(match?.prompt).toContain("Triage the new Linear issue.");
    expect(match?.prompt).toContain("Work on this Linear issue:");
    expect(match?.prompt).toContain("ENG-12 Fix auth");
  });

  it("still scopes Linear issues when they carry a project path", () => {
    const triage = automation({
      cwd: "/tmp/other",
      triggers: [createAutomationTrigger("linear", "issue_created")],
    });
    expect(
      matchInboxAutomations(
        [triage],
        [
          item({
            provider: "linear",
            kind: "linear",
            id: "issue-1",
            identifier: "ENG-12",
            url: "https://linear.app/acme/issue/ENG-12",
            projectPath: "/tmp/web",
          }),
        ],
      ),
    ).toEqual([]);
  });

  it("honors an explicit repo filter and ignores unverifiable actors", () => {
    const filtered = automation({
      triggers: [
        createAutomationTrigger("github", "pull_request_opened", {
          repos: ["acme/api"],
        }),
      ],
    });
    expect(matchInboxAutomations([filtered], [item()])).toEqual([]);
    const allowed = automation({
      id: "allowed",
      triggers: [
        createAutomationTrigger("github", "pull_request_opened", {
          repo: "acme/web",
        }),
      ],
    });
    expect(matchInboxAutomations([allowed], [item()])).toHaveLength(1);
    const authored = automation({
      id: "authored",
      triggers: [
        createAutomationTrigger("github", "pull_request_opened", {
          actor: "ada",
        }),
      ],
    });
    expect(matchInboxAutomations([authored], [item()])).toEqual([]);
  });

  it("does not launch disabled or time-only automations", () => {
    const disabled = automation({
      enabled: false,
      triggers: [createAutomationTrigger("github", "pull_request_opened")],
    });
    const scheduled = automation({
      id: "time-id",
      triggers: [createAutomationTrigger("time", "weekdays")],
    });
    expect(matchInboxAutomations([disabled, scheduled], [item()])).toEqual([]);
  });

  it("launches each automation at most once per work item", () => {
    const review = automation({
      triggers: [
        createAutomationTrigger("github", "pull_request_opened"),
        createAutomationTrigger("github", "draft_opened"),
      ],
    });
    expect(matchInboxAutomations([review], [item(), item()])).toHaveLength(1);
    expect(automationEventKey(item({ repo: "ACME/web" }))).toBe(
      "github:pr:acme/web:12",
    );
  });

  it("retries a failed backend event claim on a later poll", async () => {
    const storage = new Map<string, string>();
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        clear: () => storage.clear(),
        getItem: (key: string) => storage.get(key) ?? null,
        removeItem: (key: string) => storage.delete(key),
        setItem: (key: string, value: string) => storage.set(key, value),
      },
    });
    window.localStorage.clear();
    const review = automation({
      triggers: [createAutomationTrigger("github", "pull_request_opened")],
    });
    let rejectClaim = true;
    invoke.mockImplementation(async (command: string) => {
      if (command === "automations_list") return [review];
      if (command === "automations_claim_event") {
        if (rejectClaim) throw new Error("database busy");
        return {
          automation: review,
          run: {
            id: "run-id",
            automationId: review.id,
            trigger: "event",
            scheduledFor: at("2026-09-19T15:00:00Z"),
            createdAt: at("2026-09-19T15:00:01Z"),
            status: "pending",
          },
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });

    expect(await claimInboxAutomationRuns([item()])).toEqual([]);
    rejectClaim = false;
    const retried = await claimInboxAutomationRuns([]);

    expect(retried).toHaveLength(1);
    expect(retried[0]?.linkedWorkItem).toEqual({
      kind: "pr",
      repo: "acme/web",
      number: 12,
      url: "https://github.com/acme/web/pull/12",
    });
    expect(
      invoke.mock.calls.filter(([command]) => command === "automations_claim_event"),
    ).toHaveLength(2);
    invoke.mockReset();
    window.localStorage.clear();
  });
});
