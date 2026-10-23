import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  INBOX_CONNECTORS_CHANGE_EVENT,
  inboxConnectorEnabled,
  loadInboxDisabledConnectors,
  notifyInboxConnectorsChanged,
  saveInboxDisabledConnectors,
  setInboxConnectorEnabled,
} from "./inboxConnectors";

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("inboxConnectors", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  it("keeps every connector enabled without stored switches", () => {
    expect(loadInboxDisabledConnectors()).toEqual([]);
    for (const source of [
      "github",
      "linear",
      "jira",
      "gitlab",
      "azuredevops",
    ]) {
      expect(inboxConnectorEnabled(source)).toBe(true);
    }
  });

  it("round-trips a switched-off connector", () => {
    setInboxConnectorEnabled("github", false);
    expect(loadInboxDisabledConnectors()).toEqual(["github"]);
    expect(inboxConnectorEnabled("github")).toBe(false);
    expect(inboxConnectorEnabled("linear")).toBe(true);

    setInboxConnectorEnabled("github", true);
    expect(loadInboxDisabledConnectors()).toEqual([]);
    expect(inboxConnectorEnabled("github")).toBe(true);
  });

  it("saves nothing when every connector is enabled again", () => {
    setInboxConnectorEnabled("jira", false);
    setInboxConnectorEnabled("jira", true);
    expect(localStorage.getItem("monocode.inboxDisabledConnectors")).toBeNull();
  });

  it("announces a change when a switch moves so the Inbox can refetch", () => {
    const listener = vi.fn();
    const registry = new Map<string, () => void>();
    Object.defineProperty(globalThis, "window", {
      value: {
        addEventListener: (name: string, cb: () => void) => {
          registry.set(name, cb);
        },
        removeEventListener: (name: string) => {
          registry.delete(name);
        },
        dispatchEvent: () => {
          registry.get(INBOX_CONNECTORS_CHANGE_EVENT)?.();
          return true;
        },
      } as unknown as typeof window,
      configurable: true,
    });
    window.addEventListener(INBOX_CONNECTORS_CHANGE_EVENT, listener);
    try {
      setInboxConnectorEnabled("gitlab", false);
      expect(listener).toHaveBeenCalledTimes(1);
      notifyInboxConnectorsChanged();
      expect(listener).toHaveBeenCalledTimes(2);
    } finally {
      window.removeEventListener(INBOX_CONNECTORS_CHANGE_EVENT, listener);
    }
  });

  it("keeps saved credentials untouched: switches land in their own key", () => {
    localStorage.setItem(
      "monocode.gitlabConfig",
      JSON.stringify({ url: "https://gitlab.com", token: "secret" }),
    );
    setInboxConnectorEnabled("gitlab", false);
    expect(
      JSON.parse(localStorage.getItem("monocode.gitlabConfig") ?? "null"),
    ).toEqual({ url: "https://gitlab.com", token: "secret" });
  });
});
