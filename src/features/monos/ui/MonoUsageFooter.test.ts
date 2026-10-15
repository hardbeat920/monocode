// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  selectProviderAccount,
  saveProviderAccount,
  selectedProviderAccountId,
} from "../../providers/model/providerAccounts";
import { idleRateLimits } from "../../providers/model/rateLimits";
import {
  clearCachedRateLimits,
  setCachedRateLimits,
} from "../../providers/model/rateLimitsCache";
import { getSession } from "../../sessions/data/sessionStore";
import {
  newSession,
  type Session,
  type MonoSpawnedSession,
} from "../../sessions/model/session";
import { MonoUsageFooter } from "./MonoUsageFooter";

const providerFetch = vi.hoisted(() => ({
  claude: vi.fn(),
  codex: vi.fn(),
}));

vi.mock("../../sessions/data/sessionStore", () => ({
  getSession: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

vi.mock("../../providers/model/rateLimitsFetch", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../providers/model/rateLimitsFetch")
  >()),
  fetchClaudeRateLimits: providerFetch.claude,
  fetchCodexRateLimits: providerFetch.codex,
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
    id: "claude-project",
    provider: "claude",
    label: "Project account",
  });
  vi.mocked(getSession).mockReset();
  vi.mocked(invoke).mockClear();
  providerFetch.claude
    .mockReset()
    .mockImplementation(async () => idleRateLimits("claude"));
  providerFetch.codex
    .mockReset()
    .mockImplementation(async () => idleRateLimits("codex"));
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
  vi.useRealTimers();
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
      sessionId: "claude-legacy",
      cwd: "/repo",
      title: "Legacy Claude",
      harness: "claude",
      model: "claude:sonnet",
    },
    {
      sessionId: "claude-new",
      cwd: "/repo",
      title: "New Claude",
      harness: "claude",
      model: "claude:sonnet",
    },
    {
      sessionId: "pi-child",
      cwd: "/repo",
      title: "Pi",
      harness: "pi",
      model: "pi:anthropic/claude-sonnet-4-6",
    },
    {
      sessionId: "pi-child-duplicate",
      cwd: "/repo",
      title: "Pi duplicate",
      harness: "pi",
      model: "pi:anthropic/claude-sonnet-4-6",
    },
    {
      sessionId: "pi-codex-child",
      cwd: "/repo",
      title: "Pi Codex",
      harness: "pi",
      model: "pi:openai-codex/gpt-5.4",
    },
    {
      sessionId: "cursor-child",
      cwd: "/repo",
      title: "Cursor",
      harness: "cursor",
      model: "cursor:auto",
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
  const legacyUsedClaude = {
    ...newSession("claude", "/repo", "claude:sonnet", "auto"),
    id: "claude-legacy",
    blocks: [{ id: "old-turn", role: "user" as const, text: "existing turn" }],
  };
  const newClaude = {
    ...newSession("claude", "/repo", "claude:sonnet", "auto"),
    id: "claude-new",
  };
  selectProviderAccount("claude", "/repo", "claude-project");
  const claudeC = {
    ...newSession("claude", "/repo", "claude:sonnet", "auto"),
    id: "claude-c",
    providerAccountId: "default",
  };
  const pi = {
    ...newSession("pi", "/repo", "pi:auto", "auto"),
    id: "pi-child",
    model: "pi:anthropic/claude-sonnet-4-6",
  };
  const piDuplicate = { ...pi, id: "pi-child-duplicate" };
  const piCodex = {
    ...newSession("pi", "/repo", "pi:auto", "auto"),
    id: "pi-codex-child",
    model: "pi:openai-codex/gpt-5.4",
  };
  const cursor = {
    ...newSession("cursor", "/repo", "cursor:auto", "auto"),
    id: "cursor-child",
  };
  const unrelated = {
    ...newSession("devin", "/repo", "devin:auto", "auto"),
    id: "unrelated",
  };

  await act(async () =>
    root.render(
      createElement(MonoUsageFooter, {
        session: mono,
        sessions: [
          mono,
          claudeA,
          claudeB,
          claudeC,
          legacyUsedClaude,
          newClaude,
          pi,
          piDuplicate,
          piCodex,
          cursor,
          unrelated,
        ],
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
  ).toHaveLength(3);
  expect(
    container.querySelector('button[aria-label="Codex usage details"]'),
  ).not.toBeNull();
  expect(container.textContent).toContain("Work");
  expect(container.textContent).toContain("Default account");
  expect(container.textContent).toContain("Project account");
  expect(
    container.querySelectorAll(
      'button[aria-label="Pi · Anthropic usage details"]',
    ),
  ).toHaveLength(1);
  expect(
    container.querySelector(
      'button[aria-label="Pi · OpenAI Codex usage details"]',
    ),
  ).not.toBeNull();
  expect(container.textContent).toContain("Cursor · usage unavailable");
  expect(container.textContent).not.toContain("Devin");

  const workspaceSessions = [
    { id: "unrelated-workspace-session", providerAccountId: "default" },
  ];
  const originalWorkspaceSessions = structuredClone(workspaceSessions);
  await act(async () =>
    container
      .querySelectorAll<HTMLButtonElement>(
        'button[aria-label="Claude Code usage details"]',
      )[2]
      .click(),
  );
  await act(async () =>
    document
      .querySelector<HTMLButtonElement>(
        'button[aria-label="Switch Claude Code account"]',
      )!
      .click(),
  );
  await act(async () =>
    document
      .querySelector<HTMLButtonElement>('button[aria-label="Work"]')!
      .click(),
  );
  expect(selectedProviderAccountId("claude", "/repo")).toBe("claude-work");
  expect(workspaceSessions).toEqual(originalWorkspaceSessions);

  const changedChild = { ...pi, harness: "cursor" as const };
  await act(async () =>
    root.render(
      createElement(MonoUsageFooter, {
        session: mono,
        sessions: [
          mono,
          claudeA,
          claudeB,
          claudeC,
          legacyUsedClaude,
          newClaude,
          changedChild,
          piDuplicate,
          piCodex,
          cursor,
          unrelated,
        ],
      }),
    ),
  );
  expect(container.textContent).toContain("Cursor · usage unavailable");
  expect(container.textContent).not.toContain("Pi · usage unavailable");
});

it("keeps floating child lookups single-flight while polling", async () => {
  vi.useFakeTimers();
  let resolveFirst!: (session: Session | null) => void;
  const firstLookup = new Promise<Session | null>((resolve) => {
    resolveFirst = resolve;
  });
  const mono = {
    ...newSession("opencode", "/repo", "opencode:auto", "auto"),
    id: "floating-mono",
    blocks: [
      {
        id: "launch",
        role: "user" as const,
        text: "start child",
        monoSpawnedSessions: [
          {
            sessionId: "floating-child",
            cwd: "/repo",
            title: "Child",
            harness: "codex" as const,
            model: "codex:gpt-5",
          },
        ],
      },
    ],
  };
  const child = {
    ...newSession("codex", "/repo", "codex:gpt-5", "auto"),
    id: "floating-child",
  };
  vi.mocked(getSession)
    .mockReturnValueOnce(firstLookup)
    .mockResolvedValue(child);

  await act(async () =>
    root.render(createElement(MonoUsageFooter, { session: mono })),
  );
  expect(getSession).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(getSession).toHaveBeenCalledTimes(1);

  await act(async () => resolveFirst(child));
  expect(
    container.querySelector('button[aria-label="Codex usage details"]'),
  ).not.toBeNull();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(getSession).toHaveBeenCalledTimes(2);
});

it.each([
  ["pi:anthropic/claude-sonnet-4-6", "anthropic"],
  ["pi:openai-codex/gpt-5.4", "openai-codex"],
])(
  "renders Pi %s usage without fetching ordinary billing-provider limits",
  async (model, piProvider) => {
    clearCachedRateLimits();
    const mono = {
      ...newSession("pi", "/repo", "pi:auto", "auto"),
      id: `pi-mono-${piProvider}`,
      model,
    };

    await act(async () =>
      root.render(
        createElement(MonoUsageFooter, { session: mono, sessions: [mono] }),
      ),
    );

    expect(invoke).toHaveBeenCalledWith("fetch_pi_usage", {
      provider: piProvider,
    });
    expect(providerFetch.claude).not.toHaveBeenCalled();
    expect(providerFetch.codex).not.toHaveBeenCalled();
  },
);

it("keeps native provider reads when Pi and a native child are both in the Mono", async () => {
  clearCachedRateLimits();
  const mono = {
    ...newSession("pi", "/repo", "pi:auto", "auto"),
    id: "mixed-mono",
    model: "pi:anthropic/claude-sonnet-4-6",
    blocks: [
      {
        id: "launch",
        role: "user" as const,
        text: "start codex",
        monoSpawnedSessions: [
          {
            sessionId: "mixed-codex",
            cwd: "/repo",
            title: "Codex child",
            harness: "codex" as const,
            model: "codex:gpt-5",
          },
        ],
      },
    ],
  };
  const child = {
    ...newSession("codex", "/repo", "codex:gpt-5", "auto"),
    id: "mixed-codex",
  };

  await act(async () =>
    root.render(
      createElement(MonoUsageFooter, {
        session: mono,
        sessions: [mono, child],
      }),
    ),
  );

  expect(invoke).toHaveBeenCalledWith("fetch_pi_usage", {
    provider: "anthropic",
  });
  expect(providerFetch.claude).not.toHaveBeenCalled();
  expect(providerFetch.codex).toHaveBeenCalledTimes(1);
});
