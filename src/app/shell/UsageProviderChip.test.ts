// @vitest-environment happy-dom
// Keep this as .ts because the project test glob intentionally excludes .test.tsx.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  idleRateLimits,
  type ProviderRateLimits,
} from "../../features/providers/model/rateLimits";
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
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => undefined),
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
  vi.mocked(openUrl).mockClear();
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
    saveShowRemainingUsage(true);
    act(() =>
      root.render(
        createElement(UsageProviderChip, { limits: codexLimits(), now }),
      ),
    );

    const trigger = button("Codex usage details");
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
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

  it("reveals emails independently of account switching and hides them on reopening", async () => {
    saveMaskEmails(true);
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

it("shows scoped weekly cards after the shared weekly card and keeps the footer compact", async () => {
  const limits = {
    ...idleRateLimits("claude"),
    status: "ok" as const,
    session: { usedPercent: 20, windowMinutes: 300, resetsAt: now + 3600000 },
    weekly: { usedPercent: 30, windowMinutes: 10080, resetsAt: now + 86400000 },
    scopedWeekly: [
      {
        label: "Fable 5.1",
        usedPercent: 95,
        windowMinutes: 10080,
        resetsAt: now + 2 * 86400000,
      },
    ],
  };
  act(() => root.render(createElement(UsageProviderChip, { limits, now })));
  const trigger = button("Claude Code usage details");
  expect(trigger.textContent).not.toContain("95%");
  expect(trigger.title).toContain("Weekly · Fable 5.1: 95% used");
  await act(async () => trigger.click());
  const dialog = document.querySelector('[role="dialog"]');
  expect(
    [...dialog!.querySelectorAll("h3")].map((heading) => heading.textContent),
  ).toEqual(["5-hour limit", "Weekly limit", "Weekly · Fable 5.1"]);
  expect(
    dialog
      ?.querySelector('[aria-label="Weekly · Fable 5.1 used"]')
      ?.getAttribute("aria-valuenow"),
  ).toBe("95");
  expect(dialog?.textContent).toContain("Resets in 2d");
  act(() => saveShowRemainingUsage(true));
  expect(
    dialog
      ?.querySelector('[aria-label="Weekly · Fable 5.1 remaining"]')
      ?.getAttribute("aria-valuenow"),
  ).toBe("5");
});

it("keeps scoped and shared meters distinct in the account picker when labels match", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    const limits = {
      ...idleRateLimits("claude"),
      status: "ok" as const,
      weekly: { usedPercent: 20, windowMinutes: 10080, resetsAt: null },
      scopedWeekly: [
        {
          label: "Weekly",
          usedPercent: 75,
          windowMinutes: 10080,
          resetsAt: null,
        },
      ],
    };
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits,
          now,
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
    const accountRow = button("Main").parentElement!;
    expect(
      [...accountRow.querySelectorAll('[role="progressbar"]')].map((bar) =>
        bar.getAttribute("aria-valuenow"),
      ),
    ).toEqual(["20", "75"]);
    expect(error.mock.calls.flat().join(" ")).not.toMatch(
      /same key|unique.*key/i,
    );
  } finally {
    error.mockRestore();
  }
});

function claudeResetLimits(): ProviderRateLimits {
  return {
    ...idleRateLimits("claude"),
    status: "ok",
    resetCredits: {
      availableCount: 2,
      credits: [
        {
          id: "cedar_ember:full",
          resetType: "claudeCedar",
          status: "available",
          remainingCount: 2,
          grantedAt: null,
          expiresAt: now + 86400000,
          title: "Full reset",
          description:
            "Resets your 5-hour session limit and weekly limit. Your scheduled weekly reset stays unchanged.",
        },
      ],
    },
  };
}

it("shows Claude reset scope, expiry and remaining count without inventing extra grants", async () => {
  const onConsumeReset = vi.fn(async () => "reset" as const);
  act(() =>
    root.render(
      createElement(UsageProviderChip, {
        limits: claudeResetLimits(),
        now,
        onConsumeReset,
      }),
    ),
  );
  await act(async () => button("Claude Code usage details").click());
  expect(document.body.textContent).toContain("Limit resets");
  expect(document.body.textContent).toContain("Full reset · 2 left");
  expect(document.body.textContent).toContain(
    "5-hour session limit and weekly limit",
  );
  expect(document.body.textContent).toContain("Expires in 1d");
  expect(
    [...document.querySelectorAll("button")].filter(
      (item) => item.textContent === "Use reset",
    ),
  ).toHaveLength(1);
  await act(async () => button("Use reset").click());
  expect(onConsumeReset).not.toHaveBeenCalled();
  await act(async () => button("Cancel").click());
  expect(onConsumeReset).not.toHaveBeenCalled();
  await act(async () => button("Use reset").click());
  await act(async () => button("Confirm").click());
  expect(onConsumeReset).toHaveBeenCalledExactlyOnceWith("cedar_ember:full");
  expect(document.body.textContent).toContain("Claude Code usage was reset.");
});

it("keeps unusable and expired Claude reset offers non-interactive", async () => {
  const limits = claudeResetLimits();
  limits.resetCredits!.credits![0].status = "unavailable";
  limits.resetCredits!.credits![0].unavailableReason =
    "Available when you reach a covered usage limit.";
  const onConsumeReset = vi.fn();
  act(() =>
    root.render(
      createElement(UsageProviderChip, { limits, now, onConsumeReset }),
    ),
  );
  await act(async () => button("Claude Code usage details").click());
  expect(document.body.textContent).toContain(
    "Available when you reach a covered usage limit.",
  );
  expect(
    [...document.querySelectorAll("button")].some(
      (item) => item.textContent === "Use reset",
    ),
  ).toBe(false);
  limits.resetCredits!.credits![0].status = "available";
  limits.resetCredits!.credits![0].expiresAt = now - 1;
  act(() =>
    root.render(
      createElement(UsageProviderChip, { limits, now, onConsumeReset }),
    ),
  );
  expect(
    [...document.querySelectorAll("button")].some(
      (item) => item.textContent === "Use reset",
    ),
  ).toBe(false);
  expect(onConsumeReset).not.toHaveBeenCalled();
});

it("links to Claude for offers unavailable on the OAuth surface", async () => {
  const limits = {
    ...idleRateLimits("claude"),
    status: "ok" as const,
    resetCredits: {
      availableCount: 0,
      credits: [],
      notice: "Some reset offers are only available in Claude Web or Desktop.",
    },
  };
  act(() => root.render(createElement(UsageProviderChip, { limits, now })));
  await act(async () => button("Claude Code usage details").click());
  expect(document.body.textContent).toContain("Claude Web or Desktop");
  await act(async () => button("Manage resets in Claude").click());
  expect(openUrl).toHaveBeenCalledExactlyOnceWith(
    "https://claude.ai/settings/usage",
  );
});

it("reports unconfirmed Claude resets as errors and cancels confirmation when accounts change", async () => {
  const limits = claudeResetLimits();
  const onConsumeReset = vi.fn(async () => {
    throw new Error("Claude reset was not confirmed.");
  });
  const render = (accountId: string) =>
    act(() =>
      root.render(
        createElement(UsageProviderChip, {
          limits,
          now,
          accountId,
          onConsumeReset,
        }),
      ),
    );
  render("account-a");
  await act(async () => button("Claude Code usage details").click());
  await act(async () => button("Use reset").click());
  render("account-b");
  expect(document.body.textContent).not.toContain("Spend this reset now?");
  expect(onConsumeReset).not.toHaveBeenCalled();
  await act(async () => button("Use reset").click());
  await act(async () => button("Confirm").click());
  expect(document.body.textContent).toContain(
    "Claude reset was not confirmed.",
  );
  expect(document.body.textContent).not.toContain("usage was reset.");
});
