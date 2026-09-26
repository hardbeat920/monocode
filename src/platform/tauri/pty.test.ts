import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { spawnPty, subscribePty, trimReplay } from "./pty";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const KB = 1024;

describe("trimReplay", () => {
  it("keeps a small buffer whole", () => {
    const sizes = [KB, KB, KB];
    expect(trimReplay(sizes, 3 * KB)).toEqual({ drop: 0, bytes: 3 * KB });
  });

  it("drops oldest chunks once the byte budget is exceeded", () => {
    // Ten 32KB chunks is 320KB, over the 256KB budget.
    const sizes = Array(10).fill(32 * KB);
    const { drop, bytes } = trimReplay(sizes, 320 * KB);
    expect(drop).toBe(2);
    expect(bytes).toBe(256 * KB);
  });

  it("bounds a flood of tiny chunks by count", () => {
    const sizes = Array(250).fill(4);
    const { drop } = trimReplay(sizes, 1000);
    expect(sizes.length - drop).toBe(200);
  });

  it("keeps the newest chunk even when it alone exceeds the budget", () => {
    const sizes = [KB, 512 * KB];
    const { drop, bytes } = trimReplay(sizes, 513 * KB);
    expect(drop).toBe(1);
    expect(bytes).toBe(512 * KB);
  });

  it("never drops the only chunk", () => {
    const sizes = [512 * KB];
    expect(trimReplay(sizes, 512 * KB)).toEqual({ drop: 0, bytes: 512 * KB });
  });
});

it("waits for PTY event listeners before spawning a shell", async () => {
  let ready!: (unlisten: () => void) => void;
  const registration = new Promise<() => void>((resolve) => {
    ready = resolve;
  });
  vi.mocked(listen).mockImplementation(() => registration);
  vi.mocked(invoke).mockResolvedValue(undefined);

  const unsubscribe = subscribePty(
    "quick-test",
    () => {},
    () => {},
  );
  const aborted = new AbortController();
  const cancelledSpawn = spawnPty("cancelled", "/tmp", 80, 24, aborted.signal);
  const spawning = spawnPty("quick-test", "/tmp/project", 80, 24);
  expect(invoke).not.toHaveBeenCalled();

  aborted.abort();
  ready(() => {});
  await Promise.all([cancelledSpawn, spawning]);
  expect(invoke).toHaveBeenCalledWith("pty_spawn", {
    id: "quick-test",
    cwd: "/tmp/project",
    cols: 80,
    rows: 24,
  });
  unsubscribe();
});
