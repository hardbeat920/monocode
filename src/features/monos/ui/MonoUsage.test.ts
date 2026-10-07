// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import { usageSnapshot } from "../../agent-app/model/usageSnapshot";
import {
  providerAccounts,
  removeProviderAccount,
  renameProviderAccount,
  saveProviderAccount,
  selectProviderAccount,
  selectedProviderAccountId,
} from "../../providers/model/providerAccounts";
import { MonoUsage } from "./MonoUsage";

const usage = vi.hoisted(() => ({ snapshot: vi.fn() }));
vi.mock("../../agent-app/model/usageSnapshot", () => ({
  usageSnapshot: usage.snapshot,
}));

const snapshot = {
  snapshotAt: Date.parse("2026-10-07T12:00:00Z"),
  accounts: [
    {
      provider: "codex",
      accountId: "personal",
      accountLabel: "Personal",
      selectedForProject: true,
      selectedForSession: true,
      cliAvailable: true,
      status: "ok",
      stale: false,
      fetchedAt: Date.parse("2026-10-07T11:55:00Z"),
      updatedAt: Date.parse("2026-10-07T11:55:00Z"),
      windows: [
        {
          id: "primary",
          scope: "account",
          unit: "percent",
          usedPercent: 20,
          remainingPercent: 80,
          windowMinutes: 300,
          resetsAt: Date.parse("2026-10-07T15:00:00Z"),
        },
        {
          id: "model:gpt-5-codex",
          scope: "model:gpt-5-codex",
          unit: "percent",
          usedPercent: 40,
          remainingPercent: 60,
          windowMinutes: 10080,
          resetsAt: Date.parse("2026-10-09T12:00:00Z"),
        },
      ],
      extraUsage: null,
      credits: [],
      resetCredits: null,
    },
    {
      provider: "claude",
      accountId: "default",
      accountLabel: "Default account",
      selectedForProject: false,
      selectedForSession: false,
      cliAvailable: true,
      status: "unavailable",
      stale: false,
      fetchedAt: null,
      updatedAt: null,
      windows: [],
      extraUsage: {
        enabled: false,
        monthlyLimit: 100,
        usedCredits: 25,
        usedPercent: 25,
        currency: "USD",
        unit: "provider_credits",
      },
      credits: [],
      resetCredits: null,
    },
    {
      provider: "opencode",
      accountId: "default",
      accountLabel: "Default account",
      selectedForProject: null,
      selectedForSession: false,
      cliAvailable: true,
      status: "unsupported",
      stale: false,
      fetchedAt: null,
      updatedAt: null,
      windows: [],
      extraUsage: null,
      credits: [],
      resetCredits: null,
    },
    {
      provider: "omp",
      accountId: "default",
      accountLabel: "Default account",
      selectedForProject: null,
      selectedForSession: false,
      cliAvailable: false,
      status: "unsupported",
      stale: false,
      fetchedAt: null,
      updatedAt: null,
      windows: [],
      extraUsage: null,
      credits: [],
      resetCredits: null,
    },
  ],
} satisfies ReturnType<typeof usageSnapshot>;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  usage.snapshot.mockReset().mockReturnValue(snapshot);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

it("shows shared and model-specific windows and refreshes through the shared snapshot", () => {
  act(() =>
    root.render(
      createElement(MonoUsage, {
        session: newSession("claude", "/tmp/project"),
      }),
    ),
  );
  const details = container.querySelector("details")!;
  expect(container.textContent).toContain("Provider usage");
  expect(details.querySelector("summary")?.textContent).not.toContain(
    "80% remaining",
  );
  expect(usage.snapshot).toHaveBeenCalledExactlyOnceWith(expect.anything(), {});

  act(() => {
    details.open = true;
    details.dispatchEvent(new Event("toggle"));
  });
  expect(container.textContent).toContain("Shared · primary · 5h");
  expect(container.textContent).toContain("model:gpt-5-codex · wk");
  expect(container.textContent).toContain("80% remaining");
  expect(container.textContent).toContain("Unsupported");
  expect(container.textContent).toContain("OpenCode Go");
  expect(container.textContent).not.toContain("omp");
  expect(container.textContent).toContain("Last successful snapshot");
  const claude = [...container.querySelectorAll("section")].find((row) =>
    row.textContent?.includes("Claude Code"),
  )!;
  expect(claude.textContent).toContain("Extra usage (disabled)");
  expect(claude.textContent).not.toContain("remaining");
  expect(usage.snapshot).toHaveBeenLastCalledWith(expect.anything(), {
    refresh: true,
  });
});

it("updates account rows immediately when provider accounts change", () => {
  const source = newSession("claude", "/tmp/project");
  const baseClaude = snapshot.accounts.find(
    (account) => account.provider === "claude",
  )!;
  usage.snapshot.mockImplementation(() => ({
    ...snapshot,
    accounts: [
      ...snapshot.accounts.filter((account) => account.provider !== "claude"),
      ...providerAccounts("claude").map((account) => ({
        ...baseClaude,
        accountId: account.id,
        accountLabel: account.label,
        selectedForProject:
          selectedProviderAccountId("claude", source.cwd) === account.id,
      })),
    ],
  }));
  act(() => root.render(createElement(MonoUsage, { session: source })));
  expect(container.textContent).toContain("Default account (default)");

  act(() =>
    saveProviderAccount({ provider: "claude", id: "work", label: "Work" }),
  );
  expect(container.textContent).toContain("Work (work)");
  act(() => renameProviderAccount("claude", "work", "Team"));
  expect(container.textContent).toContain("Team (work)");
  act(() => selectProviderAccount("claude", source.cwd, "work"));
  expect(container.textContent).toContain("Team (work) · project account");
  act(() => removeProviderAccount("claude", "work"));
  expect(container.textContent).not.toContain("Team (work)");
});
