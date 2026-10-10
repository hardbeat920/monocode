import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { afterEach, expect, it, vi } from "vitest";
import { cowHelper, hostCow } from "./cow";
import type { HostStore } from "./store";

vi.mock("node:child_process", async (original) => ({
  ...(await original<typeof import("node:child_process")>()),
  spawn: vi.fn(),
}));
vi.mock("node:fs", async (original) => ({
  ...(await original<typeof import("node:fs")>()),
  existsSync: () => true,
}));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it.each(["timeout", "stdin", "output", "termination"])(
  "waits for helper exit before reporting a %s failure to removal callers",
  async (failure) => {
    vi.stubGlobal("process", { ...process, platform: "darwin" });
    vi.useFakeTimers();
    const child = Object.assign(new EventEmitter(), {
      stdin: Object.assign(new EventEmitter(), { end: vi.fn() }),
      stdout: new EventEmitter(),
      stderr: new EventEmitter(),
      kill: vi.fn(),
      pid: 123,
    });
    vi.mocked(spawn).mockReturnValue(child as ReturnType<typeof spawn>);
    let settled = false;
    const result = hostCow({ isolationDir: "/registry" } as HostStore, "cow_remove", {
      cwd: "/project",
      cowId: "copy",
    }).catch((error: Error) => {
      settled = true;
      return error;
    });
    if (failure === "timeout") await vi.advanceTimersByTimeAsync(10 * 60_000);
    else if (failure === "stdin") child.stdin.emit("error", new Error("broken pipe"));
    else if (failure === "output") child.stdout.emit("data", Buffer.alloc(16 * 1024 * 1024 + 1));
    else child.emit("error", new Error("Cannot terminate helper"));
    await Promise.resolve();
    expect(child.kill).toHaveBeenCalledTimes(failure === "termination" ? 0 : 1);
    expect(settled).toBe(false);
    child.emit("close");
    expect(await result).toBeInstanceOf(Error);
    expect(settled).toBe(true);
  },
);

it.each(["linux", "win32"])(
  "refuses CoW on %s even when an old native helper exists",
  async (platform) => {
    vi.stubGlobal("process", { ...process, platform });
    const store = { isolationDir: "/registry" } as HostStore;
    expect(cowHelper()).toBeUndefined();
    expect(await hostCow(store, "cow_capability", { cwd: "/project" })).toMatchObject({
      supported: false,
      reason: expect.stringContaining("APFS"),
    });
    expect(await hostCow(store, "cow_list", { cwd: "/project" })).toEqual([]);
    await expect(hostCow(store, "cow_create", { cwd: "/project", sessionId: "copy" })).rejects.toThrow("macOS");
    expect(spawn).not.toHaveBeenCalled();
  },
);
