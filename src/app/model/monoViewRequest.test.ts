import { describe, expect, it } from "vitest";
import { closeMonoViewAfterSessionRemoval } from "./monoViewSessionRemoval";
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

    closeMonoViewAfterSessionRemoval(
      monoViewId,
      "session-a",
      () => ({ id: "mono-a" }),
      (removedMonoId) => {
        cancelMonoViewRequest(requests, removedMonoId);
        monoViewId = null;
      },
    );
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

    closeMonoViewAfterSessionRemoval(
      monoViewId,
      "session-a",
      () => ({ id: "mono-a" }),
      (removedMonoId) => {
        cancelMonoViewRequest(requests, removedMonoId);
        monoViewId = null;
      },
    );
    loading.resolve("session-a-reopened");
    await opening;
    expect(monoViewId).toBeNull();
  });
});
