// @vitest-environment happy-dom
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MonoUsageLimitNotice } from "./MonoUsageLimitNotice";
import { newSession, type Session } from "../../sessions/model/session";
import {
  resetHarnessModelOverlays,
  setHarnessModels,
} from "../../sessions/model/models";
import { saveProviderAccount } from "../../providers/model/providerAccounts";

vi.mock("../../../integrations/harness/core/availability", () => ({
  getHarnessAvailabilitySnapshot: () => 0,
  hasProbedHarnessAvailability: () => true,
  isHarnessAvailable: () => true,
  probeHarnessAvailability: () => Promise.resolve(),
  subscribeHarnessAvailability: () => () => {},
}));
vi.mock("../../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs: () => Promise.resolve(),
}));
vi.mock("../../../shared/ui/Popover", () => ({
  Popover: ({
    children,
    role,
    "aria-label": label,
    onKeyDown,
  }: {
    children: ReactNode;
    role?: string;
    "aria-label"?: string;
    onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
  }) =>
    createElement("div", { role, "aria-label": label, onKeyDown }, children),
}));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  setHarnessModels("claude", [
    { id: "claude:current", harness: "claude", name: "Current model" },
  ]);
  setHarnessModels("codex", [
    { id: "codex:alternate", harness: "codex", name: "Alternate model" },
  ]);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  resetHarnessModelOverlays();
  vi.unstubAllGlobals();
});
const noop = () => {};
function render(
  patch: Partial<Session> = {},
  callbacks: Record<string, unknown> = {},
) {
  const session = {
    ...newSession("claude", "/tmp", "claude:current"),
    busy: false,
    usageLimit: { resetsAt: Date.now() + 3600_000 },
    ...patch,
  };
  act(() =>
    root.render(
      createElement(MonoUsageLimitNotice, {
        session,
        onModelChange: noop,
        onResume: noop,
        onResumeAtReset: noop,
        ...callbacks,
      }),
    ),
  );
}

it("opens the actual model picker and routes a different provider choice", () => {
  const onModelChange = vi.fn();
  render({}, { onModelChange });
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Choose another model"]')!
      .click(),
  );
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[role="tab"][aria-label="Codex"]')!
      .click(),
  );
  act(() =>
    container
      .querySelector<HTMLButtonElement>(
        '[role="option"][aria-label="Alternate model, Codex"]',
      )!
      .click(),
  );
  expect(onModelChange).toHaveBeenCalledWith("codex", "codex:alternate");
  expect(container.querySelector('[role="dialog"]')).toBeNull();
});

it("offers other saved accounts and routes a selected profile", () => {
  saveProviderAccount({
    id: "account-work",
    provider: "claude",
    label: "Work account",
  });
  const onAccountChange = vi.fn();
  render({ providerAccountId: "default" }, { onAccountChange });
  const picker = container.querySelector<HTMLButtonElement>(
    '[aria-label="Choose another account"]',
  )!;
  expect(container.querySelector("select")).toBeNull();
  act(() => picker.click());
  expect(
    container.querySelector(
      '[role="menu"][aria-label="Choose another account"]',
    ),
  ).not.toBeNull();
  act(() =>
    container
      .querySelector<HTMLButtonElement>(
        '[role="menuitem"][aria-label="Work account"]',
      )!
      .click(),
  );
  expect(onAccountChange).toHaveBeenCalledWith("account-work");
  expect(container.querySelector('[role="menu"]')).toBeNull();
});

it("hides account switching when no alternate profile is saved", () => {
  render({}, { onAccountChange: vi.fn() });
  expect(
    container.querySelector('[aria-label="Choose another account"]'),
  ).toBeNull();
});

it("supports cancelling and selecting accounts with the keyboard", () => {
  saveProviderAccount({
    id: "account-work",
    provider: "claude",
    label: "Work account",
  });
  saveProviderAccount({
    id: "account-other",
    provider: "claude",
    label: "Other account",
  });
  const onAccountChange = vi.fn();
  render({}, { onAccountChange });
  const trigger = container.querySelector<HTMLButtonElement>(
    '[aria-label="Choose another account"]',
  )!;
  act(() => trigger.click());
  let menu = container.querySelector('[role="menu"]')!;
  act(() =>
    menu.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(container.querySelector('[role="menu"]')).toBeNull();
  expect(onAccountChange).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(trigger);
  act(() => trigger.click());
  menu = container.querySelector('[role="menu"]')!;
  act(() =>
    menu.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    ),
  );
  act(() =>
    menu.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    ),
  );
  expect(onAccountChange).toHaveBeenCalledWith("account-other");
  expect(container.querySelector('[role="menu"]')).toBeNull();
});

it("supports arming and cancelling reset recovery, and resuming without a known reset", () => {
  const onResumeAtReset = vi.fn();
  const onResume = vi.fn();
  render({}, { onResumeAtReset });
  const reset = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent?.trim() === "Resume at reset")!;
  act(() => reset.click());
  expect(onResumeAtReset).toHaveBeenCalledWith(true);
  render(
    { usageLimit: { resetsAt: Date.now() + 3600_000, resumeAtReset: true } },
    { onResumeAtReset },
  );
  const cancel = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent?.trim() === "Resuming at reset")!;
  act(() => cancel.click());
  expect(onResumeAtReset).toHaveBeenLastCalledWith(false);
  render({ usageLimit: {} }, { onResume });
  const resume = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent?.trim() === "Resume")!;
  act(() => resume.click());
  expect(onResume).toHaveBeenCalledOnce();
  render({ usageLimit: undefined });
  expect(container.textContent).toBe("");
});
