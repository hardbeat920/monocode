// @vitest-environment happy-dom
// Keep this as .ts because the project test glob intentionally excludes .test.tsx.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { parseClaudeOAuthUsage, type ProviderRateLimits } from "../../features/providers/model/rateLimits";
import { projectKey } from "../../shared/lib/paths";
import { saveTabGroupMascot } from "../../features/workspace/model/tabGroups";
import { needsProviderLogin, UsageProviderChip } from "./UsageProviderChip";
import {
  saveMaskEmails,
  saveShowRemainingUsage,
} from "../../features/settings/model/displayPrefs";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

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
  vi.mocked(invoke).mockReset().mockResolvedValue(null);
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
  it("uses the selected model and scoped labels in the account picker", async () => {
    const limits = parseClaudeOAuthUsage(
      JSON.stringify({
        five_hour: { utilization: 10 },
        seven_day: { utilization: 20 },
        seven_day_opus: { utilization: 100 },
      }),
    );
    await act(async () =>
      root.render(
        createElement(UsageProviderChip, {
          limits,
          now,
          model: "claude:sonnet-5",
          accountId: "default",
          accounts: [
            {
              id: "default",
              provider: "claude",
              label: "Main",
              isDefault: true,
            },
          ],
          onSelectAccount: vi.fn(),
          onAddAccount: vi.fn(),
        }),
      ),
    );
    await act(async () => button("Claude Code usage details").click());
    await act(async () => button("Switch Claude Code account").click());
    const row = button("Main").parentElement!;
    expect(row.textContent).toContain("Ready");
    expect(
      row.querySelector('[aria-label="Weekly Opus limit used"]'),
    ).not.toBeNull();
    expect(row.querySelector('[aria-label="wk limit used"]')).not.toBeNull();
  });
  it("shows scoped weekly quotas and usage credit state", async () => {
    const limits = parseClaudeOAuthUsage(JSON.stringify({ five_hour: { utilization: 10 }, limits: [{ kind: "weekly_scoped", scope: { model: { display_name: "Fable 5.1" } }, percent: 91, resets_at: "2026-09-20T00:00:00Z" }], extra_usage: { is_enabled: true, utilization: 25 } }));
    await act(async () => root.render(createElement(UsageProviderChip, { limits, now, model: "claude:fable-5" })));
    await act(async () => button("Claude Code usage details").click());
    expect(document.body.textContent).toContain("Weekly Fable 5.1");
    expect(document.body.textContent).toContain("Usage credits enabled, 25% used.");
  });
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
    saveShowRemainingUsage(true);
    act(() =>
      root.render(
        createElement(UsageProviderChip, { limits: codexLimits(), now }),
      ),
    );

    const trigger = button("Codex usage details");
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(trigger.textContent).toBe("58% 2h·19% 2d 23h");
    expect(trigger.title).toBe(
      "58% remaining · Resets in 2h · 19% remaining · Resets in 2d 23h",
    );
    expect(trigger.querySelector(".w-8 > span")?.getAttribute("style")).toBe(
      "width: 19%;",
    );
    await act(async () => trigger.click());

    const dialog = document.querySelector('[role="dialog"]');
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(dialog?.textContent).toContain("5-hour limit");
    expect(dialog?.textContent).toContain("Weekly limit");
    expect(dialog?.textContent).toContain("58% remaining");
    expect(dialog?.textContent).toContain("19% remaining");
    expect(
      dialog?.querySelector("section")?.querySelector("h3")?.nextElementSibling
        ?.textContent,
    ).toBe("58% remaining");
    expect(dialog?.querySelectorAll('[role="progressbar"]')).toHaveLength(2);
    const sessionBar = dialog?.querySelector(
      '[aria-label="5-hour limit remaining"]',
    );
    const weeklyBar = dialog?.querySelector(
      '[aria-label="Weekly limit remaining"]',
    );
    expect(sessionBar?.getAttribute("aria-valuenow")).toBe("58");
    expect(sessionBar?.querySelector("span")?.getAttribute("style")).toBe(
      "width: 58%;",
    );
    expect(weeklyBar?.getAttribute("aria-valuenow")).toBe("19");
    expect(weeklyBar?.querySelector("span")?.getAttribute("style")).toBe(
      "width: 19%;",
    );
  });

  it("fills bars with used capacity by default", async () => {
    act(() =>
      root.render(
        createElement(UsageProviderChip, { limits: codexLimits(), now }),
      ),
    );

    const trigger = button("Codex usage details");
    expect(trigger.textContent).toBe("42% 2h·81% 2d 23h");
    expect(trigger.title).toBe(
      "42% used · Resets in 2h · 81% used · Resets in 2d 23h",
    );
    expect(trigger.querySelector(".w-8 > span")?.getAttribute("style")).toBe(
      "width: 81%;",
    );
    await act(async () => trigger.click());

    const weeklyBar = document.querySelector(
      '[role="dialog"] [aria-label="Weekly limit used"]',
    );
    expect(weeklyBar?.getAttribute("aria-valuenow")).toBe("81");
    expect(weeklyBar?.querySelector("span")?.getAttribute("style")).toBe(
      "width: 81%;",
    );
  });

  it.each(["this window", "another window"])(
    "updates the footer and open popover when the preference changes in %s",
    async (source) => {
      act(() =>
        root.render(
          createElement(UsageProviderChip, { limits: codexLimits(), now }),
        ),
      );
      const trigger = button("Codex usage details");
      await act(async () => trigger.click());
      const changePreference = async (remaining: boolean) => {
        await act(async () => {
          if (source === "this window") {
            saveShowRemainingUsage(remaining);
          } else {
            localStorage.setItem(
              "monocode.showRemainingUsage",
              remaining ? "1" : "0",
            );
            window.dispatchEvent(
              new StorageEvent("storage", {
                key: "monocode.showRemainingUsage",
              }),
            );
          }
        });
      };

      await changePreference(true);
      expect(trigger.textContent).toBe("58% 2h·19% 2d 23h");
      expect(trigger.title).toContain("19% remaining");
      expect(trigger.querySelector(".w-8 > span")?.getAttribute("style")).toBe(
        "width: 19%;",
      );
      const remainingBar = document.querySelector(
        '[aria-label="Weekly limit remaining"]',
      );
      expect(remainingBar?.getAttribute("aria-valuenow")).toBe("19");
      expect(remainingBar?.previousElementSibling?.textContent).toBe(
        "Weekly limit19% remaining",
      );

      await changePreference(false);
      expect(trigger.textContent).toBe("42% 2h·81% 2d 23h");
      expect(trigger.title).toContain("81% used");
      expect(trigger.querySelector(".w-8 > span")?.getAttribute("style")).toBe(
        "width: 81%;",
      );
      const usedBar = document.querySelector(
        '[aria-label="Weekly limit used"]',
      );
      expect(usedBar?.getAttribute("aria-valuenow")).toBe("81");
      expect(usedBar?.previousElementSibling?.textContent).toBe(
        "Weekly limit81% used",
      );
    },
  );

  it("shows a full bar before usage and an empty bar when exhausted", async () => {
    saveShowRemainingUsage(true);
    const limits = codexLimits();
    limits.session!.usedPercent = 0;
    limits.weekly!.usedPercent = 100;
    act(() => root.render(createElement(UsageProviderChip, { limits, now })));

    expect(
      button("Codex usage details")
        .querySelector(".w-8 > span")
        ?.getAttribute("style"),
    ).toBe("width: 0%;");
    await act(async () => button("Codex usage details").click());

    const dialog = document.querySelector('[role="dialog"]');
    const sessionBar = dialog?.querySelector(
      '[aria-label="5-hour limit remaining"]',
    );
    const weeklyBar = dialog?.querySelector(
      '[aria-label="Weekly limit remaining"]',
    );
    expect(sessionBar?.getAttribute("aria-valuenow")).toBe("100");
    expect(sessionBar?.querySelector("span")?.getAttribute("style")).toBe(
      "width: 100%;",
    );
    expect(weeklyBar?.getAttribute("aria-valuenow")).toBe("0");
    expect(weeklyBar?.querySelector("span")?.getAttribute("style")).toBe(
      "width: 0%;",
    );
  });

  it("switches between named accounts from the usage popover", async () => {
    saveShowRemainingUsage(true);
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
    const accountRow = button("Default account").parentElement!;
    const accountBar = accountRow.querySelector(
      '[aria-label="5h limit remaining"]',
    );
    expect(accountRow.textContent).toContain("58% left");
    expect(accountBar?.getAttribute("aria-valuenow")).toBe("58");
    expect(accountBar?.querySelector("span")?.getAttribute("style")).toBe(
      "width: 58%;",
    );
    await act(async () => button("Work").click());

    expect(onSelectAccount).toHaveBeenCalledWith("account-work");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("applies email masking live, reveals independently of account switching and hides on reopening", async () => {
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "provider_account_identity"
        ? { email: "user@example.com", plan: "Pro" }
        : null,
    );
    const onSelectAccount = vi.fn();
    await act(async () =>
      root.render(
        createElement(UsageProviderChip, {
          limits: codexLimits(),
          now,
          accountId: "default",
          accounts: [
            {
              id: "default",
              provider: "codex",
              label: "Main",
              isDefault: true,
            },
          ],
          onSelectAccount,
          onAddAccount: vi.fn(),
        }),
      ),
    );
    await act(async () => button("Codex usage details").click());

    expect(document.querySelector('[aria-label="Reveal email"]')).toBeNull();
    expect(document.querySelector('[title="user@example.com"]')).not.toBeNull();
    await act(async () => saveMaskEmails(true));

    const email = button("Reveal email");
    expect(email.querySelector("span")?.className).toContain("blur-[5px]");
    expect(email.querySelector("span")?.getAttribute("aria-hidden")).toBe(
      "true",
    );
    expect(document.body.textContent).toContain("Pro");
    await act(async () => email.click());
    expect(button("Hide email").querySelector("span")?.className).not.toContain(
      "blur",
    );
    expect(document.body.textContent).toContain("Codex usage");
    expect(document.body.textContent).not.toContain("Codex accounts");
    expect(onSelectAccount).not.toHaveBeenCalled();
    await act(async () => button("Hide email").click());
    expect(button("Reveal email").getAttribute("aria-pressed")).toBe("false");

    await act(async () => button("Reveal email").click());
    await act(async () => button("Codex usage details").click());
    await act(async () => button("Codex usage details").click());
    expect(button("Reveal email").getAttribute("aria-pressed")).toBe("false");

    await act(async () => button("Switch Codex account").click());
    expect(button("Reveal email").querySelector("span")?.className).toContain(
      "blur-[5px]",
    );
    expect(document.querySelector("button button")).toBeNull();
    expect(
      [...document.querySelectorAll("[title], [aria-label]")].some((element) =>
        [
          element.getAttribute("title"),
          element.getAttribute("aria-label"),
        ].some((label) => label?.includes("user@example.com")),
      ),
    ).toBe(false);
    await act(async () => button("Reveal email").click());
    expect(onSelectAccount).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Codex accounts");
    await act(async () => button("Main").click());
    expect(onSelectAccount).toHaveBeenCalledWith("default");
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

  it("uses the project's picked mascot when banked resets are available", async () => {
    const project = "/repo/mascot-lab";
    saveTabGroupMascot(projectKey(project), "cat");
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits: codexLimits(),
          now,
          project,
        }),
      ),
    );

    await act(async () => button("Codex usage details").click());
    const mascot = document.querySelector('[data-reset-mascot-mood="happy"]');
    expect(mascot?.getAttribute("data-mascot-name")).toBe("cat");
  });

  it("hides the banked resets card when no resets are available", async () => {
    const limits = codexLimits();
    limits.resetCredits = { availableCount: 0, credits: [] };
    act(() => root.render(createElement(UsageProviderChip, { limits, now })));

    await act(async () => button("Codex usage details").click());
    expect(document.body.textContent).not.toContain("Banked resets");
    expect(document.querySelector(".reset-mascot-scene")).toBeNull();
    expect(
      document.querySelector('[aria-label="Available banked resets"]'),
    ).toBeNull();
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
