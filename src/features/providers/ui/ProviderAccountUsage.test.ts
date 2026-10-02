// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { idleRateLimits } from "../model/rateLimits";
import {
  AccountUsageMeters,
  meterWindows,
  UsageMeter,
} from "./ProviderAccountUsage";

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

it("flips the meter when another window turns on remaining usage", async () => {
  const now = Date.now();
  await act(async () =>
    root.render(
      createElement(UsageMeter, {
        title: "5h",
        now,
        window: {
          usedPercent: 23,
          windowMinutes: 300,
          resetsAt: now + 3_600_000,
        },
      }),
    ),
  );
  const bar = () => container.querySelector('[role="progressbar"]');
  expect(bar()?.getAttribute("aria-label")).toBe("5h limit used");
  expect(bar()?.querySelector("span")?.getAttribute("style")).toBe(
    "width: 23%;",
  );

  await act(async () => {
    localStorage.setItem("monocode.showRemainingUsage", "1");
    window.dispatchEvent(
      new StorageEvent("storage", { key: "monocode.showRemainingUsage" }),
    );
  });

  expect(bar()?.getAttribute("aria-label")).toBe("5h limit remaining");
  expect(bar()?.querySelector("span")?.getAttribute("style")).toBe(
    "width: 77%;",
  );
  expect(container.textContent).toContain("77% left");
});

it("renders model weekly meters after the shared weekly meter", () => {
  const now = Date.now();
  const limits = {
    ...idleRateLimits("claude"),
    weekly: { usedPercent: 20, windowMinutes: 10080, resetsAt: null },
    scopedWeekly: [
      {
        label: "Fable 5.1",
        usedPercent: 75,
        windowMinutes: 10080,
        resetsAt: now + 86400000,
      },
    ],
  };
  act(() => root.render(createElement(AccountUsageMeters, { limits, now })));
  const bars = container.querySelectorAll('[role="progressbar"]');
  expect([...bars].map((bar) => bar.getAttribute("aria-label"))).toEqual([
    "Weekly limit used",
    "Fable 5.1 limit used",
  ]);
  expect(bars[1].getAttribute("aria-valuenow")).toBe("75");
  expect(container.textContent).toContain("Fable 5.1");
  expect(container.textContent).toContain("1d");
});

it("keeps shared and scoped meters distinct when their titles match", () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    const window = { usedPercent: 20, windowMinutes: 10080, resetsAt: null };
    const limits = {
      ...idleRateLimits("claude"),
      session: window,
      weekly: window,
      monthly: window,
      scopedWeekly: ["5h", "Weekly", "Monthly"].map((label) => ({
        ...window,
        label,
        usedPercent: 75,
      })),
    };
    expect(meterWindows(limits).map((entry) => entry.key)).toEqual([
      "session",
      "weekly",
      "scoped-weekly:5h",
      "scoped-weekly:Weekly",
      "scoped-weekly:Monthly",
      "monthly",
    ]);
    const render = () =>
      act(() =>
        root.render(
          createElement(AccountUsageMeters, { limits, now: Date.now() }),
        ),
      );
    render();
    limits.scopedWeekly.reverse();
    render();
    expect(
      [...container.querySelectorAll('[role="progressbar"]')].map((bar) =>
        bar.getAttribute("aria-valuenow"),
      ),
    ).toEqual(["20", "20", "75", "75", "75", "20"]);
    expect(error.mock.calls.flat().join(" ")).not.toMatch(
      /same key|unique.*key/i,
    );
  } finally {
    error.mockRestore();
  }
});
