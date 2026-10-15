// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  saveProviderAccount,
  selectedProviderAccountId,
} from "../../providers/model/providerAccounts";
import { idleRateLimits } from "../../providers/model/rateLimits";
import { setCachedRateLimits } from "../../providers/model/rateLimitsCache";
import {
  newSession,
  type MonoSpawnedSession,
} from "../../sessions/model/session";
import { MonoUsageFooter } from "./MonoUsageFooter";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    clear: () => data.clear(),
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  });
  saveProviderAccount({ id: "claude-work", provider: "claude", label: "Work" });
  saveProviderAccount({
    id: "codex-work",
    provider: "codex",
    label: "Codex Work",
  });
  setCachedRateLimits("codex", "default", {
    ...idleRateLimits("codex"),
    status: "ok",
    session: { usedPercent: 10, windowMinutes: 300, resetsAt: null },
    updatedAt: Date.now(),
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

it("shows deduplicated usage for Mono-owned providers and excludes unrelated sessions", async () => {
  const launches: MonoSpawnedSession[] = [
    {
      sessionId: "claude-a",
      cwd: "/repo",
      title: "A",
      harness: "claude",
      model: "claude:sonnet",
    },
    {
      sessionId: "claude-b",
      cwd: "/repo",
      title: "B",
      harness: "claude",
      model: "claude:sonnet",
    },
    {
      sessionId: "claude-c",
      cwd: "/repo",
      title: "C",
      harness: "claude",
      model: "claude:sonnet",
    },
    {
      sessionId: "pi-child",
      cwd: "/repo",
      title: "Pi",
      harness: "pi",
      model: "pi:auto",
    },
  ];
  const mono = {
    ...newSession("codex", "/repo", "codex:gpt-5", "auto"),
    id: "mono-session",
    blocks: [
      {
        id: "launches",
        role: "user" as const,
        text: "start",
        monoSpawnedSessions: launches,
      },
    ],
  };
  const claudeA = {
    ...newSession("claude", "/repo", "claude:sonnet", "auto"),
    id: "claude-a",
    providerAccountId: "claude-work",
  };
  const claudeB = { ...claudeA, id: "claude-b" };
  const claudeC = {
    ...newSession("claude", "/repo", "claude:sonnet", "auto"),
    id: "claude-c",
    providerAccountId: "default",
  };
  const pi = {
    ...newSession("pi", "/repo", "pi:auto", "auto"),
    id: "pi-child",
  };
  const unrelated = {
    ...newSession("devin", "/repo", "devin:auto", "auto"),
    id: "unrelated",
  };

  await act(async () =>
    root.render(
      createElement(MonoUsageFooter, {
        session: mono,
        sessions: [mono, claudeA, claudeB, claudeC, pi, unrelated],
      }),
    ),
  );

  expect(
    container.querySelector('[aria-label="Mono provider usage"]'),
  ).not.toBeNull();
  expect(
    container.querySelectorAll(
      'button[aria-label="Claude Code usage details"]',
    ),
  ).toHaveLength(2);
  expect(
    container.querySelector('button[aria-label="Codex usage details"]'),
  ).not.toBeNull();
  expect(container.textContent).toContain("Work");
  expect(container.textContent).toContain("Default account");
  expect(container.textContent).toContain("Pi · usage unavailable");
  expect(container.textContent).not.toContain("Devin");

  const workspaceSessions = [
    { id: "unrelated-workspace-session", providerAccountId: "default" },
  ];
  const originalWorkspaceSessions = structuredClone(workspaceSessions);
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>(
        'button[aria-label="Codex usage details"]',
      )!
      .click(),
  );
  await act(async () =>
    document
      .querySelector<HTMLButtonElement>(
        'button[aria-label="Switch Codex account"]',
      )!
      .click(),
  );
  await act(async () =>
    document
      .querySelector<HTMLButtonElement>('button[aria-label="Codex Work"]')!
      .click(),
  );
  expect(selectedProviderAccountId("codex", "/repo")).toBe("codex-work");
  expect(workspaceSessions).toEqual(originalWorkspaceSessions);

  const changedChild = { ...pi, harness: "cursor" as const };
  await act(async () =>
    root.render(
      createElement(MonoUsageFooter, {
        session: mono,
        sessions: [mono, claudeA, claudeB, claudeC, changedChild, unrelated],
      }),
    ),
  );
  expect(container.textContent).toContain("Cursor · usage unavailable");
  expect(container.textContent).not.toContain("Pi · usage unavailable");
});
