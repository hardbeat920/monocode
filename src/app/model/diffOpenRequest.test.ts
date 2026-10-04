import { describe, expect, it, vi } from "vitest";
import { createDiffOpenRequests } from "./diffOpenRequest";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("createDiffOpenRequests", () => {
  it("opens exact paths without waiting on the file index", () => {
    const resolve = vi.fn();
    const open = vi.fn();
    createDiffOpenRequests(resolve).open("/r", "/r/a.ts", true, open);
    expect(open).toHaveBeenCalledWith("/r/a.ts");
    expect(resolve).not.toHaveBeenCalled();
  });

  it("opens a shortened path once the index resolves it", async () => {
    const open = vi.fn();
    const requests = createDiffOpenRequests(async () => "/r/src/a.ts");
    requests.open("/r", "a.ts", false, open);
    await vi.waitFor(() => expect(open).toHaveBeenCalledWith("/r/src/a.ts"));
  });

  it("keeps a later Changes click when a delayed index lookup finishes", async () => {
    const index = deferred<string | undefined>();
    const open = vi.fn();
    const requests = createDiffOpenRequests(() => index.promise);

    requests.open("/r", "transcript.ts", false, open);
    requests.open("/r", "/r/changed.ts", true, open);
    expect(open).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenLastCalledWith("/r/changed.ts");

    index.resolve("/r/src/transcript.ts");
    await index.promise;
    await Promise.resolve();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("drops an older lookup that finishes after a newer one", async () => {
    const slow = deferred<string | undefined>();
    const fast = deferred<string | undefined>();
    const lookups = [slow, fast];
    const open = vi.fn();
    const requests = createDiffOpenRequests(() => lookups.shift()!.promise);

    requests.open("/r", "first.ts", false, open);
    requests.open("/r", "second.ts", false, open);
    fast.resolve("/r/second.ts");
    await vi.waitFor(() => expect(open).toHaveBeenCalledWith("/r/second.ts"));
    slow.resolve("/r/first.ts");
    await slow.promise;
    await Promise.resolve();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("drops a pending lookup when another view opens meanwhile", async () => {
    const index = deferred<string | undefined>();
    const open = vi.fn();
    const requests = createDiffOpenRequests(() => index.promise);

    requests.open("/r", "transcript.ts", false, open);
    requests.cancel();
    index.resolve("/r/src/transcript.ts");
    await index.promise;
    await Promise.resolve();
    expect(open).not.toHaveBeenCalled();
  });
});
