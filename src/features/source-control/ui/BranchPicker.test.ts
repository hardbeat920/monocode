// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitBranches: vi.fn(async () => ({
    current: "main",
    detached: false,
    branches: [{ name: "main", current: true, remote: null }],
  })),
  subscribeGitChanged: () => () => {},
  gitCheckout: vi.fn(),
  gitCommit: vi.fn(),
  gitCreateBranch: vi.fn(),
  gitStageAll: vi.fn(),
  gitStash: vi.fn(),
  isCheckoutBlockedByChanges: () => false,
  notifyGitChanged: vi.fn(),
}));

import { BranchPicker } from "./BranchPicker";
import { gitCreateBranch } from "../../../platform/tauri/fs";

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
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("autofocuses the branch search input only once the popover frame is visible", async () => {
  const seenVisibility: (string | undefined)[] = [];
  const nativeFocus = HTMLInputElement.prototype.focus;
  vi.spyOn(HTMLInputElement.prototype, "focus").mockImplementation(function (
    this: HTMLInputElement,
  ) {
    const frame = this.closest("[data-popover-side]")
      ?.parentElement as HTMLElement | null;
    seenVisibility.push(frame?.style.visibility);
    return nativeFocus.call(this);
  });

  act(() =>
    root.render(createElement(BranchPicker, { cwd: "/repo", branch: "main" })),
  );
  await act(async () => {});
  const button = container.querySelector("button")!;
  await act(async () => button.click());
  await act(async () => {});

  const input = document.querySelector<HTMLInputElement>(
    'input[aria-label="Search or create a branch"]',
  );
  expect(input).not.toBeNull();
  // Focusing a `visibility: hidden` element is a no-op in real browsers, so
  // the popover's pre-paint measure pass must never be where focus lands.
  expect(seenVisibility).not.toContain("hidden");
  expect(document.activeElement).toBe(input);
});

it("uses the interface typeface for branch names and search", async () => {
  act(() =>
    root.render(createElement(BranchPicker, { cwd: "/repo", branch: "main" })),
  );
  await act(async () => {});
  await act(async () => container.querySelector("button")!.click());

  const picker = document.querySelector<HTMLElement>("[data-branch-picker]")!;
  const search = picker.querySelector<HTMLInputElement>(
    'input[aria-label="Search or create a branch"]',
  )!;
  const branchName = picker.querySelector<HTMLElement>('[role="option"] span')!;

  expect(search.className).toContain("font-sans");
  expect(branchName.className).not.toContain("font-mono");
  expect(branchName.className).toContain("font-medium");
});

it("asks for a branch name before creating from the fixed action", async () => {
  act(() =>
    root.render(createElement(BranchPicker, { cwd: "/repo", branch: "main" })),
  );
  await act(async () => {});
  await act(async () => container.querySelector("button")!.click());

  const picker = document.querySelector<HTMLElement>("[data-branch-picker]")!;
  const create = [...picker.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "New branch",
  )!;
  const list = picker.querySelector('[role="listbox"][aria-label="Branches"]')!;
  expect(list.contains(create)).toBe(false);
  expect(create.parentElement?.className).toContain("border-t");
  expect(create.parentElement?.className).toContain("shrink-0");
  expect(create.parentElement).toBe(picker.lastElementChild);
  expect(create.className).toContain("hover:bg-content/8");

  await act(async () => create.click());
  expect(gitCreateBranch).not.toHaveBeenCalled();

  const input = document.querySelector<HTMLInputElement>(
    'input[aria-label="Branch name"]',
  )!;
  expect(input).not.toBeNull();
  await act(async () => {});
  expect(document.activeElement).toBe(input);

  const submit = [
    ...document.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent === "Create branch")!;
  expect(submit.disabled).toBe(true);

  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(input, "feature/picker");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(submit.disabled).toBe(false);

  await act(async () => submit.click());
  expect(gitCreateBranch).toHaveBeenCalledWith("/repo", "feature/picker");
});

it("updates the branch creation row with the entered name", async () => {
  act(() =>
    root.render(createElement(BranchPicker, { cwd: "/repo", branch: "main" })),
  );
  await act(async () => {});
  await act(async () => container.querySelector("button")!.click());

  const input = document.querySelector<HTMLInputElement>(
    'input[aria-label="Search or create a branch"]',
  )!;
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(input, "feature/picker");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });

  const picker = document.querySelector<HTMLElement>("[data-branch-picker]")!;
  const create = [...picker.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === "Create and checkout feature/picker",
  );
  expect(create).not.toBeUndefined();
  expect(create?.parentElement?.className).toContain("border-t");
});
