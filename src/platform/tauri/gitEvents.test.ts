// @vitest-environment happy-dom
import { expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { notifyGitChanged, subscribeGitChanged } from "./fs";

it("refreshes only the affected repository while preserving unscoped mutation events", () => {
  const a = vi.fn();
  const b = vi.fn();
  const all = vi.fn();
  const stopA = subscribeGitChanged(a, "/repo-a");
  const stopB = subscribeGitChanged(b, "/repo-b");
  const stopAll = subscribeGitChanged(all);
  try {
    notifyGitChanged("/repo-a");
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).not.toHaveBeenCalled();
    expect(all).toHaveBeenCalledTimes(1);
    notifyGitChanged();
    expect(a).toHaveBeenCalledTimes(2);
    expect(b).toHaveBeenCalledTimes(1);
    stopA();
    notifyGitChanged("/repo-a");
    expect(a).toHaveBeenCalledTimes(2);
    expect(b).toHaveBeenCalledTimes(1);
  } finally {
    stopA();
    stopB();
    stopAll();
  }
});
