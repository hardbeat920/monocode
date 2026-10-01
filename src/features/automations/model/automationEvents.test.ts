// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import {
  automationEventKey,
  claimInboxAutomationRuns,
  inboxAppearedEvent,
  inboxTransitionEvent,
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

describe("inbox transition events", () => {
  const issue = (overrides: Partial<InboxItem> = {}) =>
    item({
      kind: "issue",
      url: "https://github.com/acme/web/issues/12",
      ...overrides,
    });

  function stubStorage(): Map<string, string> {
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
    return storage;
  }

  function claimCalls() {
    return invoke.mock.calls
      .filter(([command]) => command === "automations_claim_event")
      .map(([, args]) => (args as { claim: Record<string, unknown> }).claim);
  }

  it("maps GitHub state changes to their own trigger events", () => {
    expect(inboxTransitionEvent(issue(), "reopened")).toEqual({
      kind: "github",
      event: "issue_reopened",
    });
    expect(inboxTransitionEvent(issue(), "closed")).toEqual({
      kind: "github",
      event: "issue_closed",
    });
    expect(inboxTransitionEvent(item(), "reopened")).toEqual({
      kind: "github",
      event: "pull_request_reopened",
    });
    expect(inboxTransitionEvent(item(), "closed")).toEqual({
      kind: "github",
      event: "pull_request_closed",
    });
    expect(inboxTransitionEvent(item(), "merged")).toEqual({
      kind: "github",
      event: "pull_request_merged",
    });
    expect(inboxTransitionEvent(item(), "ready_for_review")).toEqual({
      kind: "github",
      event: "pull_request_ready_for_review",
    });
  });

  it("has no event for changes an item cannot go through, or for other providers", () => {
    expect(inboxTransitionEvent(issue(), "merged")).toBeNull();
    expect(inboxTransitionEvent(issue(), "ready_for_review")).toBeNull();
    expect(
      inboxTransitionEvent(item({ provider: "gitlab" }), "reopened"),
    ).toBeNull();
  });

  it("keeps the opened key and gives every later change its own key", () => {
    const first = issue({ updatedAt: "2026-09-20T10:00:00Z" });
    const second = issue({ updatedAt: "2026-09-22T10:00:00Z" });
    expect(automationEventKey(first)).toBe("github:issue:acme/web:12");
    expect(automationEventKey(first, "reopened")).toBe(
      `github:issue:acme/web:12:reopened:${at("2026-09-20T10:00:00Z")}`,
    );
    expect(automationEventKey(first, "reopened")).toBe(
      automationEventKey({ ...first }, "reopened"),
    );
    expect(automationEventKey(second, "reopened")).not.toBe(
      automationEventKey(first, "reopened"),
    );
    expect(automationEventKey(first, "closed")).not.toBe(
      automationEventKey(first, "reopened"),
    );
  });

  it("fires a reopened issue into its trigger and tells the agent why", () => {
    const again = automation({
      prompt: "Look at this again.",
      triggers: [createAutomationTrigger("github", "issue_reopened")],
    });
    const reopened = issue({ updatedAt: "2026-09-22T10:00:00Z" });
    const [match] = matchInboxAutomations(
      [again],
      [],
      [{ item: reopened, transition: "reopened" }],
    );
    expect(match?.trigger.event).toBe("issue_reopened");
    expect(match?.eventKey).toBe(automationEventKey(reopened, "reopened"));
    expect(match?.occurredAt).toBe(at("2026-09-22T10:00:00Z"));
    expect(match?.prompt).toContain("Look at this again.");
    expect(match?.prompt).toContain("This GitHub issue was reopened:");
    expect(match?.prompt).toContain("#12 Fix checkout");
    expect(match?.prompt).toContain("https://github.com/acme/web/issues/12");
    expect(match?.prompt).not.toContain("Work on this GitHub issue:");
  });

  it("keeps opened and changed events apart", () => {
    const onOpen = automation({
      id: "on-open",
      triggers: [createAutomationTrigger("github", "issue_opened")],
    });
    const onReopen = automation({
      id: "on-reopen",
      triggers: [createAutomationTrigger("github", "issue_reopened")],
    });
    const both = [onOpen, onReopen];
    expect(
      matchInboxAutomations(both, [issue()]).map((match) => match.automation.id),
    ).toEqual(["on-open"]);
    expect(
      matchInboxAutomations(
        both,
        [],
        [{ item: issue(), transition: "reopened" }],
      ).map((match) => match.automation.id),
    ).toEqual(["on-reopen"]);
    expect(
      matchInboxAutomations(both, [], [{ item: issue(), transition: "closed" }]),
    ).toEqual([]);
  });

  it("separates merged pull requests from closed ones", () => {
    const onMerge = automation({
      id: "on-merge",
      triggers: [createAutomationTrigger("github", "pull_request_merged")],
    });
    const onClose = automation({
      id: "on-close",
      triggers: [createAutomationTrigger("github", "pull_request_closed")],
    });
    const merged = item({ state: "merged" });
    const [match] = matchInboxAutomations(
      [onMerge, onClose],
      [],
      [{ item: merged, transition: "merged" }],
    );
    expect(match?.automation.id).toBe("on-merge");
    expect(match?.prompt).toContain("This GitHub pull request was merged:");
    expect(
      matchInboxAutomations(
        [onMerge, onClose],
        [],
        [{ item: item({ state: "closed" }), transition: "closed" }],
      ).map((entry) => entry.automation.id),
    ).toEqual(["on-close"]);
  });

  it("fires a draft marked ready into its own trigger", () => {
    const onReady = automation({
      triggers: [
        createAutomationTrigger("github", "pull_request_ready_for_review"),
      ],
    });
    const [match] = matchInboxAutomations(
      [onReady],
      [],
      [{ item: item(), transition: "ready_for_review" }],
    );
    expect(match?.prompt).toContain(
      "This GitHub pull request was marked ready for review:",
    );
    expect(matchInboxAutomations([onReady], [item()])).toEqual([]);
  });

  it("lets one automation run for the open and for a later reopen of the same issue", () => {
    const both = automation({
      triggers: [
        createAutomationTrigger("github", "issue_opened"),
        createAutomationTrigger("github", "issue_reopened"),
      ],
    });
    const [opened] = matchInboxAutomations([both], [issue()]);
    const [reopened] = matchInboxAutomations(
      [both],
      [],
      [{ item: issue(), transition: "reopened" }],
    );
    expect(opened?.eventKey).toBe("github:issue:acme/web:12");
    expect(opened?.trigger.event).toBe("issue_opened");
    expect(reopened?.eventKey).toBe(automationEventKey(issue(), "reopened"));
    expect(reopened?.trigger.event).toBe("issue_reopened");
  });

  it("scopes changed items to the automation's project and repo filter", () => {
    const elsewhere = automation({
      cwd: "/tmp/other",
      triggers: [createAutomationTrigger("github", "issue_reopened")],
    });
    const filtered = automation({
      id: "filtered",
      triggers: [
        createAutomationTrigger("github", "issue_reopened", {
          repos: ["acme/api"],
        }),
      ],
    });
    const allowed = automation({
      id: "allowed",
      triggers: [
        createAutomationTrigger("github", "issue_reopened", {
          repos: ["acme/web"],
        }),
      ],
    });
    expect(
      matchInboxAutomations(
        [elsewhere, filtered, allowed],
        [],
        [{ item: issue(), transition: "reopened" }],
      ).map((match) => match.automation.id),
    ).toEqual(["allowed"]);
  });

  it("claims a reopened issue under its own key and retries it after a failed claim", async () => {
    stubStorage();
    const again = automation({
      triggers: [createAutomationTrigger("github", "issue_reopened")],
    });
    const reopened = issue({ updatedAt: "2026-09-22T10:00:00Z" });
    let rejectClaim = true;
    invoke.mockImplementation(async (command: string) => {
      if (command === "automations_list") return [again];
      if (command === "automations_claim_event") {
        if (rejectClaim) throw new Error("database busy");
        return {
          automation: again,
          run: {
            id: "run-id",
            automationId: again.id,
            trigger: "event",
            scheduledFor: at("2026-09-22T10:00:00Z"),
            createdAt: at("2026-09-22T10:00:01Z"),
            status: "pending",
          },
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });

    expect(
      await claimInboxAutomationRuns([], at("2026-09-22T10:00:05Z"), [
        { item: reopened, transition: "reopened" },
      ]),
    ).toEqual([]);
    rejectClaim = false;
    const retried = await claimInboxAutomationRuns([]);

    expect(retried).toHaveLength(1);
    expect(retried[0]?.prompt).toContain("This GitHub issue was reopened:");
    expect(retried[0]?.linkedWorkItem).toEqual({
      kind: "issue",
      repo: "acme/web",
      number: 12,
      url: "https://github.com/acme/web/issues/12",
    });
    const claims = claimCalls();
    expect(claims).toHaveLength(2);
    for (const claim of claims) {
      expect(claim.eventKey).toBe(automationEventKey(reopened, "reopened"));
      expect(claim.event).toBe("issue_reopened");
      expect(claim.scheduledFor).toBe(at("2026-09-22T10:00:00Z"));
    }
    expect(await claimInboxAutomationRuns([])).toEqual([]);
    expect(claimCalls()).toHaveLength(2);
    invoke.mockReset();
    window.localStorage.clear();
  });

  it("still retries opened items saved by an older version", async () => {
    const storage = stubStorage();
    storage.set(
      "monocode.automation-inbox-retries.v1",
      JSON.stringify([issue()]),
    );
    const triage = automation({
      triggers: [createAutomationTrigger("github", "issue_opened")],
    });
    invoke.mockImplementation(async (command: string) => {
      if (command === "automations_list") return [triage];
      if (command === "automations_claim_event") {
        return {
          automation: triage,
          run: {
            id: "run-id",
            automationId: triage.id,
            trigger: "event",
            scheduledFor: at("2026-09-19T15:00:00Z"),
            createdAt: at("2026-09-19T15:00:01Z"),
            status: "pending",
          },
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    vi.resetModules();
    const fresh = await import("./automationEvents");

    expect(await fresh.claimInboxAutomationRuns([])).toHaveLength(1);
    expect(claimCalls()[0]?.eventKey).toBe("github:issue:acme/web:12");
    expect(claimCalls()[0]?.event).toBe("issue_opened");
    expect(storage.has("monocode.automation-inbox-retries.v1")).toBe(false);
    invoke.mockReset();
  });
});

describe("label added events", () => {
  const label = (name: string) => ({ name, color: "ededed" });
  const issue = (overrides: Partial<InboxItem> = {}) =>
    item({
      kind: "issue",
      url: "https://github.com/acme/web/issues/12",
      updatedAt: "2026-09-22T10:00:00Z",
      ...overrides,
    });
  const stamp = at("2026-09-22T10:00:00Z");
  const onLabel = (name: string, id = "on-label") =>
    automation({
      id,
      prompt: "Fix this issue.",
      triggers: [createAutomationTrigger("github", "issue_labeled", { label: name })],
    });

  it("maps a label added to an issue or pull request to its trigger event", () => {
    expect(inboxTransitionEvent(issue(), "labeled")).toEqual({
      kind: "github",
      event: "issue_labeled",
    });
    expect(inboxTransitionEvent(item(), "labeled")).toEqual({
      kind: "github",
      event: "pull_request_labeled",
    });
  });

  it("keys a labeled event by label so two labels on one item stay distinct", () => {
    expect(automationEventKey(issue(), "labeled", "Good First Issue")).toBe(
      `github:issue:acme/web:12:labeled:good_first_issue:${stamp}`,
    );
    expect(automationEventKey(issue(), "labeled", "type: bug")).toBe(
      `github:issue:acme/web:12:labeled:type__bug:${stamp}`,
    );
    expect(automationEventKey(issue(), "labeled")).toBe(
      `github:issue:acme/web:12:labeled:${stamp}`,
    );
  });

  it("fires only for the chosen label, whatever its letter case", () => {
    const labeled = issue({ labels: [label("bug"), label("Auto-Fix")] });
    const matches = matchInboxAutomations(
      [onLabel("auto-fix")],
      [],
      [
        { item: labeled, transition: "labeled", label: "bug" },
        { item: labeled, transition: "labeled", label: "Auto-Fix" },
      ],
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]?.trigger.event).toBe("issue_labeled");
    expect(matches[0]?.eventKey).toBe(
      automationEventKey(labeled, "labeled", "Auto-Fix"),
    );
    expect(matches[0]?.prompt).toContain("Fix this issue.");
    expect(matches[0]?.prompt).toContain(
      'This GitHub issue was labeled "Auto-Fix":',
    );
    expect(matches[0]?.prompt).toContain("#12 Fix checkout");
    expect(
      matchInboxAutomations(
        [onLabel("auto-fix")],
        [],
        [{ item: labeled, transition: "labeled", label: "bug" }],
      ),
    ).toEqual([]);
  });

  it("runs an any-label trigger once when several labels are added together", () => {
    const labeled = issue({ labels: [label("bug"), label("ui")] });
    const matches = matchInboxAutomations(
      [onLabel("")],
      [],
      [
        { item: labeled, transition: "labeled", label: "bug" },
        { item: labeled, transition: "labeled", label: "ui" },
      ],
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]?.eventKey).toBe(automationEventKey(labeled, "labeled"));
  });

  it("treats the labels on a newly opened issue as added", () => {
    const opened = issue({ labels: [label("auto-fix")] });
    const [match] = matchInboxAutomations([onLabel("auto-fix")], [opened]);
    expect(match?.trigger.event).toBe("issue_labeled");
    expect(match?.eventKey).toBe(
      automationEventKey(opened, "labeled", "auto-fix"),
    );
    expect(matchInboxAutomations([onLabel("auto-fix")], [issue()])).toEqual([]);
  });

  it("runs an automation once for an issue that is opened already labeled", () => {
    const both = automation({
      triggers: [
        createAutomationTrigger("github", "issue_opened"),
        createAutomationTrigger("github", "issue_labeled", { label: "auto-fix" }),
      ],
    });
    const matches = matchInboxAutomations(
      [both],
      [issue({ labels: [label("auto-fix")] })],
    );
    expect(matches).toHaveLength(1);
    expect(matches[0]?.trigger.event).toBe("issue_opened");
  });

  it("still runs for two different changes to one item that arrive together", () => {
    const both = automation({
      triggers: [
        createAutomationTrigger("github", "issue_closed"),
        createAutomationTrigger("github", "issue_reopened"),
      ],
    });
    const closed = issue({ state: "closed", updatedAt: "2026-09-22T10:00:00Z" });
    const reopened = issue({ updatedAt: "2026-09-22T11:00:00Z" });
    expect(
      matchInboxAutomations(
        [both],
        [],
        [
          { item: closed, transition: "closed" },
          { item: reopened, transition: "reopened" },
        ],
      ).map((match) => match.trigger.event),
    ).toEqual(["issue_closed", "issue_reopened"]);
  });

  it("does not fire label triggers for a pull request when watching issues", () => {
    const labeledPr = item({ labels: [label("auto-fix")] });
    expect(
      matchInboxAutomations(
        [onLabel("auto-fix")],
        [],
        [{ item: labeledPr, transition: "labeled", label: "auto-fix" }],
      ),
    ).toEqual([]);
  });

  it("retries a failed label claim with the same label and key", async () => {
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
    const fix = onLabel("auto-fix");
    const labeled = issue({ labels: [label("auto-fix")] });
    let rejectClaim = true;
    invoke.mockImplementation(async (command: string) => {
      if (command === "automations_list") return [fix];
      if (command === "automations_claim_event") {
        if (rejectClaim) throw new Error("database busy");
        return {
          automation: fix,
          run: {
            id: "run-id",
            automationId: fix.id,
            trigger: "event",
            scheduledFor: stamp,
            createdAt: stamp + 1,
            status: "pending",
          },
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    vi.resetModules();
    const fresh = await import("./automationEvents");

    expect(
      await fresh.claimInboxAutomationRuns([], stamp, [
        { item: labeled, transition: "labeled", label: "auto-fix" },
      ]),
    ).toEqual([]);
    rejectClaim = false;
    vi.resetModules();
    const reloaded = await import("./automationEvents");
    const retried = await reloaded.claimInboxAutomationRuns([]);

    expect(retried).toHaveLength(1);
    const claims = invoke.mock.calls
      .filter(([command]) => command === "automations_claim_event")
      .map(([, args]) => (args as { claim: Record<string, unknown> }).claim);
    expect(claims).toHaveLength(2);
    for (const claim of claims) {
      expect(claim.event).toBe("issue_labeled");
      expect(claim.eventKey).toBe(
        automationEventKey(labeled, "labeled", "auto-fix"),
      );
    }
    invoke.mockReset();
    storage.clear();
  });
});

