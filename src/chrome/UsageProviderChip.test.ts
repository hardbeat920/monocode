// @vitest-environment happy-dom
// Keep this as .ts because the project test glob intentionally excludes .test.tsx.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRateLimits } from "../lib/rateLimits";
import { projectKey } from "../lib/paths";
import { saveTabGroupMascot } from "../lib/tabGroups";
import { needsProviderLogin, UsageProviderChip } from "./UsageProviderChip";

const now = Date.parse("2026-09-16T12:00:00Z");

function codexLimits(): ProviderRateLimits {
  return {
    provider: "codex",
    session: {
      usedPercent: 42,
      windowMinutes: 300,
      resetsAt: now + 2 * 3_600_000,
    },
    weekly: {
      usedPercent: 81,
      windowMinutes: 10_080,
      resetsAt: now + 2 * 86_400_000 + 23 * 3_600_000,
    },
    monthly: null,
    resetCredits: {
      availableCount: 2,
      credits: [
        {
          id: "reset-1",
          resetType: "codexRateLimits",
          status: "available",
          grantedAt: now - 86_400_000,
          expiresAt: now + 12 * 86_400_000,
          title: "Referral reward",
          description: "One Codex rate-limit reset",
        },
        {
          id: "reset-2",
          resetType: "codexRateLimits",
          status: "available",
          grantedAt: now - 43_200_000,
          expiresAt: now + 18 * 86_400_000,
          title: "Backup reset",
          description: "A second Codex rate-limit reset",
        },
      ],
    },
    updatedAt: now,
    error: null,
    status: "ok",
  };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

function button(label: string): HTMLButtonElement {
  const result = [
    ...document.querySelectorAll<HTMLButtonElement>("button"),
  ].find(
    (item) => (item.getAttribute("aria-label") ?? item.textContent) === label,
  );
  expect(result, label).toBeDefined();
  return result!;
}

describe("UsageProviderChip", () => {
  it("offers the provider-owned login flow for an expired Claude session", async () => {
    const limits: ProviderRateLimits = {
      provider: "claude",
      session: null,
      weekly: null,
      monthly: null,
      resetCredits: null,
      updatedAt: now,
      error: "Claude sign-in expired",
      status: "error",
    };
    const onReconnect = vi.fn(async () => undefined);
    act(() =>
      root.render(
        createElement(UsageProviderChip, { limits, now, onReconnect }),
      ),
    );

    await act(async () => button("Claude Code usage details").click());
    expect(document.querySelector(".size-9")).not.toBeNull();
    await act(async () => button("Sign in to Claude Code").click());

    expect(onReconnect).toHaveBeenCalledOnce();
    expect(document.body.textContent).toContain("Signed in to Claude Code");
  });

  it("does not describe account-specific usage restrictions as login failures", () => {
    const limits: ProviderRateLimits = {
      provider: "claude",
      session: null,
      weekly: null,
      monthly: null,
      resetCredits: null,
      updatedAt: now,
      error: "Claude usage is unavailable for this account",
      status: "error",
    };
    expect(needsProviderLogin(limits)).toBe(false);
  });

  it("opens a column of detailed progress bars", async () => {
    act(() =>
      root.render(
        createElement(UsageProviderChip, { limits: codexLimits(), now }),
      ),
    );

    const trigger = button("Codex usage details");
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    await act(async () => trigger.click());

    const dialog = document.querySelector('[role="dialog"]');
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(dialog?.textContent).toContain("5-hour limit");
    expect(dialog?.textContent).toContain("Weekly limit");
    expect(dialog?.textContent).toContain("58% remaining");
    expect(dialog?.textContent).toContain("19% remaining");
    expect(dialog?.querySelectorAll('[role="progressbar"]')).toHaveLength(2);
    expect(
      dialog
        ?.querySelector('[aria-label="Weekly limit used"]')
        ?.getAttribute("aria-valuenow"),
    ).toBe("81");
  });

  it("switches between named accounts from the usage popover", async () => {
    const onSelectAccount = vi.fn();
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits: codexLimits(),
          now,
          accountId: "default",
          accounts: [
            {
              id: "default",
              provider: "codex",
              label: "Default account",
              isDefault: true,
            },
            { id: "account-work", provider: "codex", label: "Work" },
          ],
          onSelectAccount,
          onAddAccount: vi.fn(),
        }),
      ),
    );

    await act(async () => button("Codex usage details").click());
    await act(async () => button("Switch Codex account").click());
    expect(document.body.textContent).toContain("Codex accounts");
    expect(document.body.textContent).toContain("Default account");
    await act(async () => button("Work").click());

    expect(onSelectAccount).toHaveBeenCalledWith("account-work");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("keeps account switching available when the pinned account was removed", async () => {
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits: codexLimits(),
          now,
          accountId: "account-missing",
          accounts: [
            {
              id: "default",
              provider: "codex",
              label: "Default account",
              isDefault: true,
            },
            { id: "account-work", provider: "codex", label: "Work" },
          ],
          onSelectAccount: vi.fn(),
          onAddAccount: vi.fn(),
        }),
      ),
    );

    const trigger = button("Codex usage details");
    expect(trigger.textContent).not.toContain("Default account");
    await act(async () => trigger.click());
    expect(document.body.textContent).toContain("Removed account");
    await act(async () => button("Switch Codex account").click());
    expect(document.body.textContent).toContain("Default account");
    expect(document.body.textContent).toContain("Work");
  });

  it("shows and deliberately consumes a banked reset", async () => {
    const onConsumeReset = vi.fn(async () => "reset" as const);
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits: codexLimits(),
          now,
          onConsumeReset,
        }),
      ),
    );

    await act(async () => button("Codex usage details").click());
    expect(document.body.textContent).toContain("2 resets available");
    expect(
      document.querySelector('[data-reset-mascot-mood="happy"]'),
    ).not.toBeNull();
    expect(document.body.textContent).toContain("Referral reward");
    expect(document.body.textContent).toContain("Backup reset");
    expect(document.body.textContent).toContain("Expires in 12d");

    const list = document.querySelector(
      '[aria-label="Available banked resets"]',
    );
    expect(list?.classList.contains("overflow-y-auto")).toBe(true);
    const useButtons = [
      ...document.querySelectorAll<HTMLButtonElement>("button"),
    ].filter((item) => item.textContent === "Use reset");
    expect(useButtons).toHaveLength(2);
    act(() => useButtons[1]?.click());
    expect(document.body.textContent).toContain("Spend this reset now?");
    await act(async () => button("Confirm").click());

    expect(onConsumeReset).toHaveBeenCalledWith("reset-2");
    expect(document.body.textContent).toContain("Codex usage was reset.");
  });

  it("uses the project's picked mascot and gives an empty bank a sad pose", async () => {
    const project = "/repo/mascot-lab";
    saveTabGroupMascot(projectKey(project), "cat");
    const limits = codexLimits();
    limits.resetCredits = { availableCount: 0, credits: [] };
    act(() =>
      root.render(createElement(UsageProviderChip, { limits, now, project })),
    );

    await act(async () => button("Codex usage details").click());
    const mascot = document.querySelector('[data-reset-mascot-mood="sad"]');
    expect(mascot?.getAttribute("data-mascot-name")).toBe("cat");
    expect(document.body.textContent).toContain("No resets available");
  });

  it("keeps aggregate-only resets visible as claimable rows", async () => {
    const limits = codexLimits();
    limits.resetCredits = {
      availableCount: 2,
      credits: limits.resetCredits?.credits?.slice(0, 1) ?? null,
    };
    const onConsumeReset = vi.fn(async () => "reset" as const);
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits,
          now,
          onConsumeReset,
        }),
      ),
    );

    await act(async () => button("Codex usage details").click());
    expect(document.body.textContent).toContain("Referral reward");
    expect(document.body.textContent).toContain("Banked reset 2");
    const useButtons = [
      ...document.querySelectorAll<HTMLButtonElement>("button"),
    ].filter((item) => item.textContent === "Use reset");
    expect(useButtons).toHaveLength(2);
  });
});
