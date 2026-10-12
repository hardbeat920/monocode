// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageFooter, workspaceUsageAccountSelection } from "./UsageFooter";
import {
  saveProviderAccount,
  selectedProviderAccountId,
  type ProviderAccountProvider,
} from "../../features/providers/model/providerAccounts";
import {
  clearCachedRateLimits,
  setCachedRateLimits,
} from "../../features/providers/model/rateLimitsCache";
import { idleRateLimits } from "../../features/providers/model/rateLimits";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

let container: HTMLDivElement;
let root: Root;

async function selectClaudeAccount(
  onSelectAccount?: (provider: ProviderAccountProvider, id: string) => void,
) {
  await act(async () =>
    root.render(
      createElement(UsageFooter, {
        providers: ["claude"],
        project: "/mono-project",
        onSelectAccount,
      }),
    ),
  );
  const button = (label: string) =>
    [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === label,
    )!;
  await act(async () => button("Claude Code usage details").click());
  await act(async () => button("Switch Claude Code account").click());
  await act(async () => button("Work").click());
}

describe("UsageFooter terminal control", () => {
  it("replaces the generic terminal button with the live process control", () => {
    const markup = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        terminals: [
          {
            id: "terminal-1",
            process: "npm",
            cwd: "/repo",
            label: "repo",
          },
        ],
        terminalOpen: true,
        onToggleTerminal: vi.fn(),
        onNewTerminal: vi.fn(),
        onShowTerminal: vi.fn(),
        projectTerminalActive: true,
      }),
    );

    expect(markup).toContain(">npm</span>");
    expect(markup).not.toContain(">Terminal</span>");
    expect(markup.match(/<button/g)).toHaveLength(1);
  });

  it("keeps the generic terminal button when no process is running", () => {
    const markup = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        onNewTerminal: vi.fn(),
      }),
    );

    expect(markup).toContain(">Terminal</span>");
    expect(markup.match(/<button/g)).toHaveLength(1);
  });
});

describe("UsageFooter account selection", () => {
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const data = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      clear: () => data.clear(),
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => data.set(key, value),
      removeItem: (key: string) => data.delete(key),
    });
    localStorage.clear();
    clearCachedRateLimits();
    setCachedRateLimits("claude", "default", {
      ...idleRateLimits("claude"),
      status: "ok",
      updatedAt: Date.now(),
    });
    saveProviderAccount({
      id: "claude-work",
      provider: "claude",
      label: "Work",
    });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    clearCachedRateLimits();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("changes Mono project usage without changing the unrelated workspace session", async () => {
    const workspaceSessions = [
      { id: "workspace-session", providerAccountId: "default" },
    ];
    const originalSessions = structuredClone(workspaceSessions);
    const onSelectProviderAccount = vi.fn(
      (_provider: ProviderAccountProvider, accountId: string) => {
        workspaceSessions[0].providerAccountId = accountId;
      },
    );

    await selectClaudeAccount(
      workspaceUsageAccountSelection(true, onSelectProviderAccount),
    );

    expect(selectedProviderAccountId("claude", "/mono-project")).toBe(
      "claude-work",
    );
    expect(onSelectProviderAccount).not.toHaveBeenCalled();
    expect(workspaceSessions).toEqual(originalSessions);
  });

  it("keeps the workspace account callback for the ordinary footer", async () => {
    const onSelectAccount = vi.fn();

    await selectClaudeAccount(
      workspaceUsageAccountSelection(false, onSelectAccount),
    );

    expect(onSelectAccount).toHaveBeenCalledWith("claude", "claude-work");
  });
});
