import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAccountUsage } from "@/features/models/useAccountUsage";

const desktop = vi.hoisted(() => ({
  command: vi.fn(),
  fingerprint: "desktop",
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({
    command: desktop.command,
    pairing: { desktop: { fingerprint: desktop.fingerprint } },
  }),
}));
(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
const response = (label: string) => ({
  accounts: {
    default: {
      status: { tone: "ready", label, detail: null },
      updatedAt: 1,
      stale: false,
      meters: [],
    },
  },
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("mobile usage refresh", () => {
  let root: ReturnType<typeof createRoot>;
  let usage: ReturnType<typeof useAccountUsage>;
  function Probe({ provider }: { provider: string }) {
    usage = useAccountUsage("project", provider);
    return null;
  }
  const render = (provider = "codex") =>
    act(async () => {
      root.render(<Probe provider={provider} />);
      await Promise.resolve();
    });
  beforeEach(() => {
    vi.clearAllMocks();
    desktop.fingerprint = "desktop";
    desktop.command.mockResolvedValue(response("Ready"));
    root = createRoot(document.createElement("div"));
  });
  afterEach(() => act(() => root.unmount()));

  it("keeps the last snapshot visible during a forced refresh and avoids duplicate probes", async () => {
    await render();
    expect(desktop.command).toHaveBeenCalledWith("accounts.usage", {
      projectId: "project",
      provider: "codex",
    });
    const fresh = deferred<ReturnType<typeof response>>();
    desktop.command.mockReturnValue(fresh.promise);
    act(() => {
      usage.refresh();
      usage.refresh();
    });
    expect(desktop.command).toHaveBeenCalledTimes(2);
    expect(desktop.command).toHaveBeenLastCalledWith("accounts.usage", {
      projectId: "project",
      provider: "codex",
      refresh: true,
    });
    expect(usage.accounts.default.status.label).toBe("Ready");
    expect(usage.refreshing).toBe(true);
    await act(async () => {
      fresh.resolve(response("Updated"));
    });
    expect(usage.accounts.default.status.label).toBe("Updated");
    expect(usage.refreshing).toBe(false);
  });

  it("retains usage and exposes retry when the desktop cannot be reached", async () => {
    await render();
    desktop.command.mockRejectedValue(new Error("Connection lost"));
    await act(async () => usage.refresh());
    expect(usage.accounts.default.status.label).toBe("Ready");
    expect(usage.error).toBe("Could not refresh usage. Try again.");
    expect(usage.refreshing).toBe(false);
    desktop.command.mockResolvedValue(response("Recovered"));
    await act(async () => usage.refresh());
    expect(usage.error).toBeNull();
    expect(usage.accounts.default.status.label).toBe("Recovered");
  });

  it("forces a fresh probe after a manual refresh during the first load", async () => {
    const first = deferred<ReturnType<typeof response>>();
    desktop.command.mockReturnValueOnce(first.promise);
    await render();
    act(() => usage.refresh());
    expect(desktop.command).toHaveBeenCalledTimes(1);
    await act(async () => {
      first.resolve(response("First"));
    });
    expect(desktop.command).toHaveBeenCalledTimes(2);
    expect(desktop.command).toHaveBeenLastCalledWith("accounts.usage", {
      projectId: "project",
      provider: "codex",
      refresh: true,
    });
    expect(usage.refreshing).toBe(false);
  });

  it("ignores late replies after changing provider or paired desktop", async () => {
    const old = deferred<ReturnType<typeof response>>();
    desktop.command.mockReturnValueOnce(old.promise);
    await render();
    await render("claude");
    expect(usage.accounts.default.status.label).toBe("Ready");
    await act(async () => old.resolve(response("Old Codex account")));
    expect(usage.accounts.default.status.label).toBe("Ready");
    desktop.fingerprint = "different-desktop";
    desktop.command.mockReturnValue(new Promise(() => {}));
    await render("claude");
    expect(usage.accounts).toEqual({});
  });

  it("explains an older desktop's missing support and skips providers without accounts", async () => {
    desktop.command.mockRejectedValue(
      new Error("Unsupported mobile operation"),
    );
    await render();
    expect(usage.error).toBe("Update the desktop app to show account usage.");
    desktop.command.mockClear();
    await render("pi");
    expect(desktop.command).not.toHaveBeenCalled();
    expect(usage.refreshing).toBe(false);
  });
});
