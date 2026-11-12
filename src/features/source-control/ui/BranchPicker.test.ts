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
  isCheckoutBlockedByChanges: vi.fn(() => false),
  notifyGitChanged: vi.fn(),
}));

vi.mock("../../files/model/fileWatch", () => ({
  invalidateWatchedFiles: vi.fn(),
}));

import { BranchPicker } from "./BranchPicker";
import {
  gitBranches,
  gitCheckout,
  gitCreateBranch,
  gitStash,
  isCheckoutBlockedByChanges,
  notifyGitChanged,
} from "../../../platform/tauri/fs";
import { invalidateWatchedFiles } from "../../files/model/fileWatch";
import { acquireRepoLock, repoLock } from "../model/repoLock";

const lockKind = (cwd: string) => repoLock(cwd)?.kind ?? null;

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

it("checks out the highlighted matching branch when Enter is pressed", async () => {
  vi.mocked(gitBranches).mockResolvedValueOnce({
    current: "main",
    detached: false,
    branches: [
      { name: "main", current: true, remote: null },
      { name: "feature/picker", current: false, remote: null },
    ],
  });
  act(() =>
    root.render(
      createElement(BranchPicker, {
        cwd: "/repo-enter-existing",
        branch: "main",
      }),
    ),
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
    setter.call(input, "picker");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });

  const picker = document.querySelector<HTMLElement>("[data-branch-picker]")!;
  const highlighted = picker.querySelector<HTMLButtonElement>(
    '[role="option"]',
  )!;
  expect(highlighted.textContent).toContain("feature/picker");
  expect(
    [...picker.querySelectorAll<HTMLButtonElement>("button")].some(
      (button) => button.textContent === "Create and checkout picker",
    ),
  ).toBe(true);

  await act(async () => {
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  });

  expect(gitCheckout).toHaveBeenCalledWith(
    "/repo-enter-existing",
    "feature/picker",
    null,
  );
  expect(gitCreateBranch).not.toHaveBeenCalledWith(
    "/repo-enter-existing",
    "picker",
  );
});

async function openPickerWithFeature(
  cwd: string,
  props: Record<string, unknown> = {},
) {
  vi.mocked(gitBranches).mockResolvedValueOnce({
    current: "main",
    detached: false,
    branches: [
      { name: "main", current: true, remote: null },
      { name: "feature/picker", current: false, remote: null },
    ],
  });
  act(() =>
    root.render(createElement(BranchPicker, { cwd, branch: "main", ...props })),
  );
  await act(async () => {});
  await act(async () => container.querySelector("button")!.click());
  return [
    ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ].find((option) => option.textContent?.includes("feature/picker"))!;
}

function buttonNamed(label: string) {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent?.trim() === label,
  )!;
}

it("holds the repository lock for the whole checkout", async () => {
  let finish!: () => void;
  vi.mocked(gitCheckout).mockImplementationOnce(
    () =>
      new Promise<string>((resolve) => {
        finish = () => resolve("");
      }),
  );
  const option = await openPickerWithFeature("/repo-busy");
  expect(lockKind("/repo-busy")).toBeNull();

  await act(async () => option.click());
  expect(lockKind("/repo-busy")).toBe("checkout");

  await act(async () => finish());
  expect(lockKind("/repo-busy")).toBeNull();
});

it("stays disabled while another holder has the repository locked", async () => {
  const release = acquireRepoLock("/repo-held", "commit")!;
  act(() =>
    root.render(
      createElement(BranchPicker, { cwd: "/repo-held", branch: "main" }),
    ),
  );
  await act(async () => {});
  const trigger = container.querySelector("button")!;
  expect(trigger.disabled).toBe(true);
  await act(async () => trigger.click());
  expect(document.querySelector("[data-branch-picker]")).toBeNull();

  act(() => release());
  expect(trigger.disabled).toBe(false);
});

it("reloads open editors after switching branches", async () => {
  vi.mocked(gitCheckout).mockResolvedValueOnce("");
  vi.mocked(invalidateWatchedFiles).mockClear();
  vi.mocked(notifyGitChanged).mockClear();
  const option = await openPickerWithFeature("/repo-reload");

  await act(async () => option.click());

  expect(gitCheckout).toHaveBeenCalledWith(
    "/repo-reload",
    "feature/picker",
    null,
  );
  expect(notifyGitChanged).toHaveBeenCalled();
  // No paths: every open editor may now show a different file.
  expect(invalidateWatchedFiles).toHaveBeenCalledWith();
});

it("reloads open editors after stashing and switching, even if the switch then fails", async () => {
  vi.mocked(isCheckoutBlockedByChanges).mockReturnValue(true);
  vi.mocked(gitCheckout)
    .mockRejectedValueOnce(new Error("would be overwritten"))
    .mockRejectedValueOnce(new Error("checkout failed"))
    .mockResolvedValueOnce("");
  vi.mocked(gitStash).mockResolvedValue("");
  try {
    const option = await openPickerWithFeature("/repo-stash");
    await act(async () => option.click());
    vi.mocked(invalidateWatchedFiles).mockClear();

    // The stash lands but the switch fails: the stash still moved files.
    await act(async () => buttonNamed("Stash & switch").click());
    expect(gitStash).toHaveBeenCalledTimes(1);
    expect(invalidateWatchedFiles).toHaveBeenCalledWith();
    expect(lockKind("/repo-stash")).toBeNull();

    vi.mocked(invalidateWatchedFiles).mockClear();
    await act(async () => buttonNamed("Stash & switch").click());
    expect(gitStash).toHaveBeenCalledTimes(2);
    expect(invalidateWatchedFiles).toHaveBeenCalledWith();
    expect(
      document.querySelector('[aria-label="Switch to feature/picker"]'),
    ).toBeNull();
    expect(lockKind("/repo-stash")).toBeNull();
  } finally {
    vi.mocked(isCheckoutBlockedByChanges).mockReturnValue(false);
  }
});

type Dismissal = "Escape" | "outside click" | "trigger click";

async function dismissPicker(how: Dismissal, trigger: HTMLButtonElement) {
  await act(async () => {
    if (how === "Escape") {
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    } else if (how === "outside click") {
      document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    } else {
      trigger.click();
    }
  });
  expect(document.querySelector("[data-branch-picker]")).toBeNull();
}

const DISMISSALS: Dismissal[] = ["Escape", "outside click", "trigger click"];

it.each(DISMISSALS)(
  "keeps the checkout lock after %s until Git settles",
  async (how) => {
    let finish!: () => void;
    vi.mocked(gitCheckout).mockClear();
    vi.mocked(gitCheckout).mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finish = () => resolve("");
        }),
    );
    const cwd = `/repo-dismiss-${how}`;
    const option = await openPickerWithFeature(cwd);
    const trigger = container.querySelector("button")!;
    await act(async () => option.click());
    expect(lockKind(cwd)).toBe("checkout");

    await dismissPicker(how, trigger);
    expect(lockKind(cwd)).toBe("checkout");

    // Reopening can't start a second checkout while the first runs.
    expect(trigger.disabled).toBe(true);
    await act(async () => trigger.click());
    expect(document.querySelector("[data-branch-picker]")).toBeNull();
    expect(gitCheckout).toHaveBeenCalledTimes(1);

    await act(async () => finish());
    expect(lockKind(cwd)).toBeNull();
    expect(trigger.disabled).toBe(false);
  },
);

it("keeps the lock for a checkout that outlives the picker", async () => {
  let finish!: () => void;
  vi.mocked(gitCheckout).mockImplementationOnce(
    () =>
      new Promise<string>((resolve) => {
        finish = () => resolve("");
      }),
  );
  const option = await openPickerWithFeature("/repo-unmount");
  await act(async () => option.click());
  act(() => root.render(null));
  expect(lockKind("/repo-unmount")).toBe("checkout");

  await act(async () => finish());
  expect(lockKind("/repo-unmount")).toBeNull();
});
