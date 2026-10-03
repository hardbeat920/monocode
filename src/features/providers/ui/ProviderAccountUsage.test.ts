// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AccountUsageMeters, UsageMeter } from "./ProviderAccountUsage";
import { parseClaudeOAuthUsage } from "../model/rateLimits";

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

it("renders model-scoped weekly limits in the account row", async () => {
  const limits = parseClaudeOAuthUsage(
    JSON.stringify({
      limits: [
        {
          kind: "weekly_scoped",
          scope: { model: { display_name: "Fable 5.1" } },
          percent: 73,
          resets_at: "2026-10-05T00:00:00Z",
        },
      ],
    }),
  );
  await act(async () =>
    root.render(createElement(AccountUsageMeters, { limits, now: Date.now() })),
  );
  expect(
    container.querySelector('[role="progressbar"]')?.getAttribute("aria-label"),
  ).toBe("Weekly Fable 5.1 limit used");
  expect(container.textContent).toContain("73%");
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
