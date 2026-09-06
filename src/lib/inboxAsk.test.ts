import { describe, expect, it } from "vitest";
import { inboxAskKey, inboxAskPrompt } from "./inboxAsk";
import type { InboxItem } from "./githubTasks";
import { newSession } from "./session";
import { shouldPersistSession, upsertSession } from "./sessionStore";
import {
  collectWorkspaceSnapshot,
  hydrateWorkspaceSnapshot,
  parseWorkspaceSnapshot,
} from "./workspaceSnapshot";
import { newTab } from "./layout";
import { inFlightRefs, workspaceFromResumed } from "./inFlight";

import { historyWithLiveSessions, summaryFromSession } from "./sessionHistory";
import { liveAgentsFromSessions } from "./liveAgents";
import { hiddenApprovalNotices } from "./approvalToast";

const context = {
  key: "github:github.com:/acme/app/pull/42",
  title: "Fix login",
  url: "https://github.com/acme/app/pull/42",
  provider: "github" as const,
};

describe("inbox sessions", () => {
  it("identifies the item independently of its local project and URL fragment", () => {
    const item = { provider: "github", url: context.url } as InboxItem;
    expect(inboxAskKey(item)).toBe(
      inboxAskKey({
        ...item,
        url: `${context.url}/#discussion`,
        projectPath: "/other",
      }),
    );
    expect(inboxAskKey(item)).not.toBe(
      inboxAskKey({ ...item, url: "https://git.example.com/acme/app/pull/42" }),
    );
    expect(
      inboxAskKey({ provider: "linear", id: "issue-uuid" } as InboxItem),
    ).toBe("linear:issue-uuid");
  });

  it("adds the remote-access instruction on follow-ups and leaves ordinary sessions alone", () => {
    expect(inboxAskPrompt(undefined, "Change the file")).toBe(
      "Change the file",
    );
    for (const text of ["Explain the PR", "What about tests?"]) {
      const prompt = inboxAskPrompt(context, text);
      expect(prompt).toContain(context.url);
      expect(prompt).toContain("Do not clone repositories");
      expect(prompt).toContain("fetch PR branches into local git");
      expect(prompt).toContain("gh pr diff");
      expect(prompt.endsWith(text)).toBe(true);
    }
  });

  it("never persists Ask, including real messages and a running provider thread", async () => {
    const session = {
      ...newSession("codex", "/project"),
      inboxAsk: context,
      busy: true,
      providerSessionId: "provider-thread",
      blocks: [{ id: "u1", role: "user" as const, text: "Explain the PR" }],
    };
    expect(shouldPersistSession(session)).toBe(false);
    expect(await upsertSession(session)).toBeNull();
    expect(inFlightRefs([session], [])).toEqual([]);
    expect(workspaceFromResumed([session])).toBeNull();
  });

  it("keeps temporary conversations out of history and live-agent lists even when awaiting approval", () => {
    const session = {
      ...newSession("codex", "/project"),
      inboxAsk: context,
      busy: true,
      blocks: [
        { id: "u1", role: "user" as const, text: "Explain the PR" },
        { id: "approval", role: "tool" as const, text: "Run command", approval: { requestId: 7 } },
      ],
    };
    const staleHistory = [summaryFromSession(session)];
    expect(historyWithLiveSessions(staleHistory, [session], session.cwd)).toEqual([]);
    expect(liveAgentsFromSessions([session], new Set([session.id]))).toEqual([]);
    expect(hiddenApprovalNotices([session], "", [], true)).toEqual([]);
  });

  it("omits Ask from workspace snapshots and restart recovery", () => {
    const project = newSession("codex", "/project");
    const session = { ...newSession("codex", "/other"), inboxAsk: context, busy: true };
    const tab = newTab(project.id);
    const snapshot = collectWorkspaceSnapshot([tab], [project, session], tab.id, project.cwd);
    expect(snapshot.sessions.map(entry => entry.id)).toEqual([project.id]);
    const restored = hydrateWorkspaceSnapshot(
      snapshot,
      new Map([[session.id, session]]),
      new Set([session.id]),
    )!;
    expect(restored.tabs).toEqual([tab]);
    expect(restored.activeTabId).toBe(tab.id);
    expect(restored.projectCwd).toBe(project.cwd);
    expect(restored.sessions.map(entry => entry.id)).toEqual([project.id]);
    expect(workspaceFromResumed([project, session])!.sessions).toEqual([project]);
  });

  it("drops Ask tabs and stubs saved by the earlier implementation", () => {
    const project = newSession("codex", "/project");
    const session = { ...newSession("codex", "/project"), inboxAsk: context };
    const tab = newTab(project.id);
    const askTab = newTab(session.id);
    const legacy = {
      tabs: [tab, askTab],
      sessions: [project, session],
      activeTabId: askTab.id,
      projectCwd: project.cwd,
      projectTerminals: [],
    };
    const parsed = parseWorkspaceSnapshot(legacy)!;
    expect(parsed.tabs).toEqual([tab]);
    expect(parsed.activeTabId).toBe(tab.id);
    expect(parsed.sessions.map(entry => entry.id)).toEqual([project.id]);
    expect(parseWorkspaceSnapshot({ ...legacy, tabs: [askTab], sessions: [session] })).toBeNull();
  });
});
