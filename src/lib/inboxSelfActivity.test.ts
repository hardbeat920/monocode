import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearPendingInboxSelfActivity,
  consumeInboxSelfActivity,
  recordInboxSelfActivity,
  subscribeInboxSelfActivity,
} from "./inboxSelfActivity";

const githubPr = {
  provider: "github" as const,
  kind: "pr" as const,
  repo: "acme/app",
  number: 42,
  projectPath: "/tmp/app",
};

afterEach(clearPendingInboxSelfActivity);

describe("self-authored Inbox activity", () => {
  it("coalesces exact mutations into one acknowledged revision", () => {
    recordInboxSelfActivity({
      provider: "github",
      kind: "pr",
      repo: "ACME/App",
      number: 42,
    });
    recordInboxSelfActivity({
      provider: "github",
      kind: "pr",
      repo: "acme/app",
      number: 42,
    });
    expect(consumeInboxSelfActivity(githubPr)).toBe(true);
    expect(consumeInboxSelfActivity(githubPr)).toBe(false);
  });

  it("does not consume activity for another item", () => {
    recordInboxSelfActivity({
      provider: "github",
      kind: "issue",
      number: 42,
      projectPath: "/tmp/app",
    });
    expect(consumeInboxSelfActivity(githubPr)).toBe(false);
  });

  it("matches Linear mutations by id", () => {
    recordInboxSelfActivity({
      provider: "linear",
      kind: "linear",
      id: "lin-7",
    });
    expect(
      consumeInboxSelfActivity({
        provider: "linear",
        kind: "linear",
        id: "lin-7",
        repo: "ENG",
        number: 7,
        projectPath: "linear:team",
      }),
    ).toBe(true);
  });

  it("expires a mutation instead of hiding unrelated later activity", () => {
    recordInboxSelfActivity(githubPr, 1_000);
    expect(consumeInboxSelfActivity(githubPr, 1_000 + 10 * 60_000 + 1)).toBe(
      false,
    );
  });

  it("notifies the poller as soon as a mutation succeeds", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeInboxSelfActivity(listener);
    recordInboxSelfActivity(githubPr);
    expect(listener).toHaveBeenCalledOnce();
    unsubscribe();
  });
});
