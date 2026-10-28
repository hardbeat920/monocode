// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { RATE_LIMIT_POLL_MS } from "../../features/providers/model/rateLimits";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { UsageFooter } from "./UsageFooter";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
let container: HTMLDivElement;
let root: Root;
const quota = (percent: number) => ({
  status: "ok",
  windows: {
    session: { usedPercent: percent, windowMinutes: 300, resetsAt: null },
    weekly: null,
  },
});

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  vi.mocked(invoke).mockReset().mockResolvedValue(quota(24));
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function show(model: string, id = "pi-session") {
  await act(async () =>
    root.render(
      createElement(UsageFooter, {
        providers: [],
        session: { id, harness: "pi", model },
      }),
    ),
  );
}

it("renders Pi-owned quotas without fetching another CLI account", async () => {
  await show("pi:anthropic/claude-sonnet-4-6");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("fetch_pi_usage", {
    provider: "anthropic",
  });
  expect(container.textContent).toContain("24%");
  const trigger = container.querySelector<HTMLButtonElement>(
    '[aria-label="Pi · Anthropic usage details"]',
  );
  expect(trigger).not.toBeNull();
  await act(async () => trigger?.click());
  expect(document.body.textContent).toContain("Pi's saved OAuth account");
  expect(document.body.textContent).not.toContain("Switch");
  expect(document.body.textContent).not.toContain("Add account");
  expect(document.body.textContent).not.toContain("Sign in");
});

it("clears old usage on provider changes and ignores late responses", async () => {
  let finishOld: ((value: unknown) => void) | undefined;
  vi.mocked(invoke).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishOld = resolve;
      }),
  );
  await show("pi:anthropic/claude-sonnet-4-6");
  vi.mocked(invoke).mockResolvedValueOnce(quota(32));
  await show("pi:openai-codex/gpt-5.4");
  expect(container.textContent).toContain("32%");
  await act(async () => finishOld?.(quota(99)));
  expect(container.textContent).not.toContain("99%");
  expect(container.textContent).toContain("32%");
  expect(invoke).toHaveBeenLastCalledWith("fetch_pi_usage", {
    provider: "openai-codex",
  });
});

it("does not guess a provider for defaults or unsupported models", async () => {
  for (const model of [
    "pi:default",
    "pi:openai/gpt-5.4",
    "pi:openrouter/anthropic/claude",
  ]) {
    await show(model);
    expect(container.textContent).toContain("pi");
    expect(container.textContent).not.toMatch(/\d+%/);
  }
  expect(invoke).not.toHaveBeenCalled();
});

it("clears quotas on failed refresh and can recover after a Pi login", async () => {
  await show("pi:anthropic/claude-sonnet-4-6");
  expect(container.textContent).toContain("24%");
  vi.mocked(invoke).mockResolvedValueOnce({
    status: "unavailable",
    message: "Sign in through Pi, then refresh usage.",
  });
  const refresh = () =>
    container.querySelector<HTMLButtonElement>(
      '[aria-label="Refresh Pi usage"]',
    );
  expect(refresh()).not.toBeNull();
  await act(async () => refresh()?.click());
  expect(container.textContent).not.toContain("24%");
  vi.mocked(invoke).mockResolvedValueOnce(quota(12));
  await act(async () => refresh()?.click());
  expect(container.textContent).toContain("12%");
});

it("isolates A to B to A and same-provider session changes", async () => {
  let finishOld: ((value: unknown) => void) | undefined;
  vi.mocked(invoke).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishOld = resolve;
      }),
  );
  await show("pi:anthropic/claude", "a");
  await show("pi:openai-codex/gpt", "b");
  vi.mocked(invoke).mockResolvedValueOnce(quota(17));
  await show("pi:anthropic/claude", "a");
  await act(async () => finishOld?.(quota(99)));
  expect(container.textContent).toContain("17%");
  expect(container.textContent).not.toContain("99%");
  vi.mocked(invoke).mockResolvedValueOnce(quota(8));
  await show("pi:anthropic/claude", "c");
  expect(container.textContent).toContain("8%");
  expect(invoke).toHaveBeenCalledTimes(4);
});

it("ignores a disposed Strict Mode request without blocking its replacement", async () => {
  let finishOld: ((value: unknown) => void) | undefined;
  vi.mocked(invoke).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishOld = resolve;
      }),
  );
  await act(async () =>
    root.render(
      createElement(
        StrictMode,
        null,
        createElement(UsageFooter, {
          providers: [],
          session: {
            id: "strict",
            harness: "pi",
            model: "pi:anthropic/claude",
          },
        }),
      ),
    ),
  );
  expect(container.textContent).toContain("24%");
  await act(async () => finishOld?.(quota(99)));
  expect(container.textContent).not.toContain("99%");
  expect(container.textContent).toContain("24%");
});

it("refreshes on native window focus after the minimum interval", async () => {
  vi.useFakeTimers();
  await show("pi:anthropic/claude");
  await act(async () => vi.advanceTimersByTimeAsync(5 * 60_000));
  vi.mocked(invoke).mockResolvedValueOnce(quota(13));
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(container.textContent).toContain("13%");
  expect(invoke).toHaveBeenCalledTimes(2);
});

it("polls only while visible and retries unavailable credentials", async () => {
  vi.useFakeTimers();
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  await show("pi:anthropic/claude");
  expect(invoke).not.toHaveBeenCalled();
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  vi.mocked(invoke).mockResolvedValueOnce({
    status: "unavailable",
    message: "Sign in through Pi.",
  });
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  expect(invoke).toHaveBeenCalledTimes(1);
  await act(async () => vi.advanceTimersByTimeAsync(RATE_LIMIT_POLL_MS));
  expect(invoke).toHaveBeenCalledTimes(2);
  expect(container.textContent).toContain("24%");
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  await act(async () => vi.advanceTimersByTimeAsync(RATE_LIMIT_POLL_MS));
  expect(invoke).toHaveBeenCalledTimes(2);
});
