import { expect, it, vi } from "vitest";
import { submitAfterProjectSync } from "./submissionAcceptance";

it("waits for a moved project's state to be rebased before submitting", async () => {
  let resolveMove!: () => void;
  const applyLocationChange = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        resolveMove = resolve;
      }),
  );
  const submit = vi.fn(() => true);
  const accepted = submitAfterProjectSync({
    cwd: "/old-path",
    sync: Promise.resolve({ path: "/new-path", identity: "repo", moved: true }),
    applyLocationChange,
    submit,
    onError: vi.fn(),
  });
  await Promise.resolve();
  expect(applyLocationChange).toHaveBeenCalledWith("/old-path", "/new-path");
  expect(submit).not.toHaveBeenCalled();
  resolveMove();
  await expect(accepted).resolves.toBe(true);
  expect(submit).toHaveBeenCalledOnce();
});

it("propagates an asynchronous deferred rejection instead of treating its promise as acceptance", async () => {
  const accepted = submitAfterProjectSync({
    cwd: "/repo",
    sync: Promise.resolve({ path: "/repo", identity: "repo", moved: false }),
    applyLocationChange: vi.fn(),
    submit: async () => false,
    onError: vi.fn(),
  });
  await expect(accepted).resolves.toBe(false);
});

it("rejects acceptance if applying a moved project fails", async () => {
  const error = new Error("failed to rebase project state");
  const submit = vi.fn(() => true);
  const onError = vi.fn();
  const accepted = submitAfterProjectSync({
    cwd: "/old-path",
    sync: Promise.resolve({ path: "/new-path", identity: "repo", moved: true }),
    applyLocationChange: vi.fn().mockRejectedValue(error),
    submit,
    onError,
  });
  await expect(accepted).resolves.toBe(false);
  expect(submit).not.toHaveBeenCalled();
  expect(onError).toHaveBeenCalledWith(error);
});

it("cancels pending project preparation and ignores its late result", async () => {
  let finishSync!: (location: {
    path: string;
    identity: string;
    moved: boolean;
  }) => void;
  const controller = new AbortController();
  const submit = vi.fn(() => true);
  const accepted = submitAfterProjectSync({
    cwd: "/repo",
    sync: new Promise((resolve) => (finishSync = resolve)),
    applyLocationChange: vi.fn(),
    submit,
    onError: vi.fn(),
    signal: controller.signal,
  });

  controller.abort();
  await expect(accepted).resolves.toBe(false);
  finishSync({ path: "/repo", identity: "repo", moved: false });
  await Promise.resolve();
  expect(submit).not.toHaveBeenCalled();
});
