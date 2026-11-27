// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useMobileWorkspace } from "./useMobileWorkspace";

const desktop = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  catalog: vi.fn(),
  usage: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: desktop.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: desktop.listen }));
vi.mock("../model/mobileWorkspace", () => ({ mobileCatalog: desktop.catalog }));
vi.mock("../model/mobileAccountUsage", () => ({
  mobileAccountUsage: desktop.usage,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const catalog = (id: string) => ({
  projects: [],
  monos: [],
  sessions: [{ id }],
});

describe("new mobile session publication", () => {
  let root: ReturnType<typeof createRoot>;
  let request: (event: { payload: object }) => void;
  const command = vi.fn();
  const flush = () =>
    act(async () => {
      for (let i = 0; i < 12; i++) await Promise.resolve();
    });
  const send = (id = "request") =>
    request({
      payload: {
        id,
        commandId: "same-start",
        method: "session.start",
        params: { text: "Hello" },
      },
    });
  function Probe() {
    useMobileWorkspace({ sessions: [], command });
    return null;
  }
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    command.mockResolvedValue({ sessionId: "new" });
    desktop.catalog.mockResolvedValue(catalog("new"));
    desktop.invoke.mockImplementation(async (method: string) =>
      method === "mobile_status" ? { enabled: false } : undefined,
    );
    desktop.listen.mockImplementation(
      async (event: string, handler: typeof request) => {
        if (event === "monocode-mobile-request") request = handler;
        return () => {};
      },
    );
    root = createRoot(document.createElement("div"));
  });
  afterEach(() => {
    act(() => root.unmount());
    vi.useRealTimers();
  });

  it("routes account usage reads and refreshes to the shared desktop usage cache", async () => {
    desktop.usage.mockResolvedValue({ accounts: {} });
    act(() => root.render(createElement(Probe)));
    request({
      payload: {
        id: "usage",
        commandId: "refresh",
        method: "accounts.usage",
        params: { projectId: "p", provider: "codex", refresh: true },
      },
    });
    await flush();
    expect(desktop.usage).toHaveBeenCalledWith("codex", true);
    expect(command).not.toHaveBeenCalled();
    expect(desktop.invoke).toHaveBeenCalledWith("mobile_reply", {
      id: "usage",
      response: { ok: true, result: { accounts: {} } },
    });
  });

  it("makes a new session routable before returning its ID to the phone", async () => {
    const published = deferred<void>();
    desktop.invoke.mockImplementation(async (method: string) => {
      if (method === "mobile_status") return { enabled: false };
      if (method === "mobile_publish") return published.promise;
    });
    act(() => root.render(createElement(Probe)));
    send();
    await flush();
    expect(desktop.invoke).toHaveBeenCalledWith("mobile_publish", {
      catalog: catalog("new"),
    });
    expect(
      desktop.invoke.mock.calls.filter(([method]) => method === "mobile_reply"),
    ).toHaveLength(0);

    published.resolve();
    await flush();
    expect(desktop.invoke).toHaveBeenCalledWith("mobile_reply", {
      id: "request",
      response: { ok: true, result: { sessionId: "new" } },
    });
  });

  it("finishes an older catalog publication before publishing the created session", async () => {
    const older = deferred<object>();
    desktop.invoke.mockImplementation(async (method: string) =>
      method === "mobile_status" ? { enabled: true } : undefined,
    );
    desktop.catalog.mockReturnValueOnce(older.promise);
    act(() => root.render(createElement(Probe)));
    await flush();
    send();
    await flush();
    expect(desktop.catalog).toHaveBeenCalledTimes(1);
    expect(
      desktop.invoke.mock.calls.filter(([method]) => method === "mobile_reply"),
    ).toHaveLength(0);

    older.resolve(catalog("old"));
    await flush();
    const publications = desktop.invoke.mock.calls.filter(
      ([method]) => method === "mobile_publish",
    );
    expect(
      publications.map(([, params]) => params.catalog.sessions[0].id),
    ).toEqual(["old", "new"]);
    expect(desktop.invoke).toHaveBeenLastCalledWith("mobile_reply", {
      id: "request",
      response: { ok: true, result: { sessionId: "new" } },
    });
  });

  it("retries a failed publication without creating the session twice", async () => {
    let publications = 0;
    desktop.invoke.mockImplementation(async (method: string) => {
      if (method === "mobile_status") return { enabled: false };
      if (method === "mobile_publish" && ++publications === 1)
        throw new Error("Publish failed");
    });
    act(() => root.render(createElement(Probe)));
    send("first");
    await flush();
    expect(desktop.invoke).toHaveBeenCalledWith("mobile_reply", {
      id: "first",
      response: { ok: false, error: "Publish failed" },
    });
    send("retry");
    await flush();
    expect(command).toHaveBeenCalledTimes(1);
    expect(desktop.invoke).toHaveBeenCalledWith("mobile_reply", {
      id: "retry",
      response: { ok: true, result: { sessionId: "new" } },
    });
  });
});
