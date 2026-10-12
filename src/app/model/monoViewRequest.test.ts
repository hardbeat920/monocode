import { describe, expect, it } from "vitest";
import { shouldCloseMonoViewAfterSessionRemoval } from "./monoViewSessionRemoval";
import {
  beginMonoViewRequest,
  cancelMonoViewRequest,
  finishMonoViewRequest,
  isMonoViewRequestCurrent,
  type MonoViewRequestState,
} from "./monoViewRequest";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("Mono view requests", () => {
  it("lets a pending switch finish when the current Mono is removed", async () => {
    const requests: MonoViewRequestState = { current: 0 };
    const loading = deferred<string>();
    let monoViewId: string | null = "session-a";

    const request = beginMonoViewRequest(requests, "mono-b");
    const opening = loading.promise.then((sessionId) => {
      if (isMonoViewRequestCurrent(requests, request)) monoViewId = sessionId;
      finishMonoViewRequest(requests, request);
    });

    if (shouldCloseMonoViewAfterSessionRemoval(monoViewId, "session-a")) {
      cancelMonoViewRequest(requests, "mono-a");
      monoViewId = null;
    }
    expect(monoViewId).toBeNull();

    loading.resolve("session-b");
    await opening;
    expect(monoViewId).toBe("session-b");
  });

  it("cancels a pending reopen of the Mono being removed", async () => {
    const requests: MonoViewRequestState = { current: 0 };
    const loading = deferred<string>();
    let monoViewId: string | null = "session-a";

    const request = beginMonoViewRequest(requests, "mono-a");
    const opening = loading.promise.then((sessionId) => {
      if (isMonoViewRequestCurrent(requests, request)) monoViewId = sessionId;
      finishMonoViewRequest(requests, request);
    });

    if (shouldCloseMonoViewAfterSessionRemoval(monoViewId, "session-a")) {
      cancelMonoViewRequest(requests, "mono-a");
      monoViewId = null;
    }
    loading.resolve("session-a-reopened");
    await opening;
    expect(monoViewId).toBeNull();
  });
});
