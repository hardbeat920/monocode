import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: Array<{
  args: { id: string; url: string };
  resolve: (value: { outcome: string }) => void;
  reject: (reason: unknown) => void;
}> = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(
    (_command: string, args: { id: string; url: string }) =>
      new Promise((resolve, reject) => calls.push({ args, resolve, reject })),
  ),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { navigateBrowser } from "./browser";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("navigateBrowser", () => {
  beforeEach(() => {
    calls.length = 0;
  });

  it("hands a tab's navigations to the native side one at a time, in order", async () => {
    const first = navigateBrowser("t1", "https://a.test/#a");
    const second = navigateBrowser("t1", "https://a.test/#b");
    const third = navigateBrowser("t1", "https://a.test/#a");
    await flush();
    expect(calls.map((c) => c.args.url)).toEqual(["https://a.test/#a"]);
    calls[0].resolve({ outcome: "applied" });
    await first;
    await flush();
    expect(calls.map((c) => c.args.url)).toEqual([
      "https://a.test/#a",
      "https://a.test/#b",
    ]);
    calls[1].resolve({ outcome: "applied" });
    await second;
    await flush();
    calls[2].resolve({ outcome: "document" });
    await expect(third).resolves.toEqual({ outcome: "document" });
    expect(calls.map((c) => c.args.url)).toEqual([
      "https://a.test/#a",
      "https://a.test/#b",
      "https://a.test/#a",
    ]);
  });

  it("goes on after a rejected navigation", async () => {
    const first = navigateBrowser("t2", "https://a.test/");
    const second = navigateBrowser("t2", "https://b.test/");
    await flush();
    calls[0].reject("blocked");
    await expect(first).rejects.toBe("blocked");
    await flush();
    expect(calls).toHaveLength(2);
    calls[1].resolve({ outcome: "document" });
    await expect(second).resolves.toEqual({ outcome: "document" });
  });

  it("does not make one tab wait for another", async () => {
    void navigateBrowser("t3", "https://a.test/");
    void navigateBrowser("t4", "https://b.test/");
    await flush();
    expect(calls.map((c) => c.args.id).sort()).toEqual(["t3", "t4"]);
  });
});
