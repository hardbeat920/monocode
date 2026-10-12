import { describe, expect, it, vi } from "vitest";

import { createCommitCache } from "./commitCache";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe("createCommitCache", () => {
  it("does not let a request started before clear() repopulate the cache", async () => {
    const first = deferred<string>();
    const fetchValue = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce("second");

    const cache = createCommitCache<string>(fetchValue);
    const pending = cache.load("/repo", "sha");
    cache.clear();
    first.resolve("first");

    // The value still reaches the caller that asked for it.
    await expect(pending).resolves.toBe("first");
    // But it must not be written into the cache that was just cleared.
    expect(cache.peek("/repo", "sha")).toBeUndefined();

    await expect(cache.load("/repo", "sha")).resolves.toBe("second");
    expect(cache.peek("/repo", "sha")).toBe("second");
  });

  it("does not let a stale request delete a newer in-flight entry", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const fetchValue = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const cache = createCommitCache<string>(fetchValue);
    const stale = cache.load("/repo", "sha");
    cache.clear();
    const live = cache.load("/repo", "sha");

    first.resolve("first");
    await stale;

    // The stale request must not have removed the newer entry, or a third
    // load would start a duplicate Git call instead of reusing the in-flight one.
    expect(cache.load("/repo", "sha")).toBe(live);
    expect(fetchValue).toHaveBeenCalledTimes(2);
  });
});
