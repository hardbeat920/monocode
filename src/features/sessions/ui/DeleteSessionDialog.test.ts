// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { DeleteSessionDialog } from "./DeleteSessionDialog";

it("shows confirmation while workspace eligibility is pending and ignores results after cancellation", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  let finish!: (path: string | undefined) => void;
  const loadUnusedWorktree = vi.fn(
    () =>
      new Promise<string | undefined>((resolve) => {
        finish = resolve;
      }),
  );
  const onClose = vi.fn();
  const props = { title: "Planning", cow: true, loadUnusedWorktree, onClose };
  let mounted = true;
  try {
    await act(async () =>
      root.render(createElement(DeleteSessionDialog, props)),
    );
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(document.body.textContent).toContain(
      "Checking workspace cleanup availability",
    );
    expect(document.querySelector('input[type="checkbox"]')).toBeNull();
    const cancel = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === "Cancel",
    )!;
    await act(async () => cancel.click());
    expect(onClose).toHaveBeenCalledWith({
      confirmed: false,
      deleteWorktree: false,
    });
    await act(async () => root.unmount());
    mounted = false;
    await act(async () => finish("/copy"));
    expect(onClose).toHaveBeenCalledTimes(1);
  } finally {
    if (mounted) await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});

it.each([false, true])(
  "offers cleanup only after eligibility succeeds (cow=%s)",
  async (cow) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    const root = createRoot(container);
    let finish!: (path: string | undefined) => void;
    const loadUnusedWorktree = () =>
      new Promise<string | undefined>((resolve) => {
        finish = resolve;
      });
    const onClose = vi.fn();
    try {
      await act(async () =>
        root.render(
          createElement(DeleteSessionDialog, {
            title: "Planning",
            cow,
            loadUnusedWorktree,
            onClose,
          }),
        ),
      );
      await act(async () => finish("/copy"));
      expect(document.querySelector('[role="status"]')).toBeNull();
      const checkbox = document.querySelector<HTMLInputElement>(
        'input[type="checkbox"]',
      )!;
      expect(checkbox.checked).toBe(false);
      expect(document.body.textContent).toContain(
        cow ? "unused copy-on-write workspace" : "unused worktree",
      );
      await act(async () => checkbox.click());
      const confirm = [...document.querySelectorAll("button")].find(
        (button) => button.textContent === "Delete session",
      )!;
      await act(async () => confirm.click());
      expect(onClose).toHaveBeenCalledWith({
        confirmed: true,
        deleteWorktree: true,
      });
    } finally {
      await act(async () => root.unmount());
      vi.unstubAllGlobals();
    }
  },
);

it.each(["pending", "unavailable", "failed"])(
  "allows session deletion without cleanup when eligibility is %s",
  async (state) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    const root = createRoot(container);
    const loadUnusedWorktree = () =>
      state === "pending"
        ? new Promise<string | undefined>(() => {})
        : state === "failed"
          ? Promise.reject(new Error("Host unavailable"))
          : Promise.resolve(undefined);
    const onClose = vi.fn();
    try {
      await act(async () =>
        root.render(
          createElement(DeleteSessionDialog, {
            title: "Planning",
            cow: true,
            loadUnusedWorktree,
            onClose,
          }),
        ),
      );
      expect(document.querySelector('input[type="checkbox"]')).toBeNull();
      const confirm = [...document.querySelectorAll("button")].find(
        (button) => button.textContent === "Delete session",
      )!;
      await act(async () => confirm.click());
      expect(onClose).toHaveBeenCalledWith({
        confirmed: true,
        deleteWorktree: false,
      });
    } finally {
      await act(async () => root.unmount());
      vi.unstubAllGlobals();
    }
  },
);
