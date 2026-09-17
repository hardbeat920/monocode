// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../lib/fs", () => ({
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
  vi.spyOn(HTMLInputElement.prototype, "focus").mockImplementation(
    function (this: HTMLInputElement) {
      const frame = this.closest("[data-popover-side]")
        ?.parentElement as HTMLElement | null;
      seenVisibility.push(frame?.style.visibility);
      return nativeFocus.call(this);
    },
  );

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
