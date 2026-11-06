// @vitest-environment happy-dom
import { act, createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
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
let root: Root | null = null;

async function selectClaudeAccount(
  onSelectAccount?: (provider: ProviderAccountProvider, id: string) => void,
) {
  await act(async () =>
    root!.render(
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
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = null;
});

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  container.remove();
  clearCachedRateLimits();
  if (typeof localStorage !== "undefined") localStorage.clear();
  vi.unstubAllGlobals();
});

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
        onToggleProjectTerminal: vi.fn(),
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

  it("marks the terminal button pressed only while the dock is open", () => {
    const open = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        onNewTerminal: vi.fn(),
        onToggleProjectTerminal: vi.fn(),
        projectTerminalActive: true,
        projectTerminalOpen: true,
      }),
    );
    const collapsed = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        onNewTerminal: vi.fn(),
        onToggleProjectTerminal: vi.fn(),
        projectTerminalActive: true,
        projectTerminalOpen: false,
      }),
    );

    // Label still reflects "has terminals" in both states.
    expect(open).toContain('aria-label="Terminal"');
    expect(collapsed).toContain('aria-label="Terminal"');
    expect(open).toContain('aria-pressed="true"');
    expect(collapsed).toContain('aria-pressed="false"');
  });

  it("toggles the dock instead of only showing it", () => {
    const onToggleProjectTerminal = vi.fn();
    const onNewTerminal = vi.fn();
    root = createRoot(container);
    act(() =>
      root!.render(
        createElement(UsageFooter, {
          providers: [],
          onNewTerminal,
          onToggleProjectTerminal,
          projectTerminalActive: true,
          projectTerminalOpen: true,
        }),
      ),
    );

    const button = container.querySelector('button[aria-label="Terminal"]');
    expect(button).not.toBeNull();
    act(() => (button as HTMLButtonElement).click());
    expect(onToggleProjectTerminal).toHaveBeenCalledTimes(1);
    expect(onNewTerminal).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "handles a toggle-only caller when projectTerminalActive is %s",
    (projectTerminalActive) => {
      const onToggleProjectTerminal = vi.fn();
      root = createRoot(container);
      act(() =>
        root!.render(
          createElement(UsageFooter, {
            providers: [],
            onToggleProjectTerminal,
            projectTerminalActive,
          }),
        ),
      );

      const button = container.querySelector("button");
      expect(button).not.toBeNull();
      act(() => button!.click());
      expect(onToggleProjectTerminal).toHaveBeenCalledTimes(1);
    },
  );

  it("prefers creating a terminal when no project terminal is active", () => {
    const onNewTerminal = vi.fn();
    const onToggleProjectTerminal = vi.fn();
    root = createRoot(container);
    act(() =>
      root!.render(
        createElement(UsageFooter, {
          providers: [],
          onNewTerminal,
          onToggleProjectTerminal,
          projectTerminalActive: false,
        }),
      ),
    );

    const button = container.querySelector("button");
    expect(button).not.toBeNull();
    act(() => button!.click());
    expect(onNewTerminal).toHaveBeenCalledTimes(1);
    expect(onToggleProjectTerminal).not.toHaveBeenCalled();
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
    root = createRoot(container);
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
