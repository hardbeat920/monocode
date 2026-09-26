// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  REMOTE_CONTROL_CHANGE_EVENT,
  REMOTE_CONTROL_DEFAULT,
  loadRemoteControl,
  saveRemoteControl,
  subscribeRemoteControl,
} from "./settings";

const KEY = "monocode.remoteControl";

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => values.delete(key),
    setItem: (key, value) => {
      values.set(key, value);
    },
  };
}

beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("remote control", () => {
  it("defaults to manual so nothing spawns a process per session unasked", () => {
    expect(REMOTE_CONTROL_DEFAULT).toBe("manual");
    expect(loadRemoteControl()).toBe("manual");
  });

  it.each(["manual", "all"] as const)("round-trips %s", (mode) => {
    saveRemoteControl(mode);
    expect(localStorage.getItem(KEY)).toBe(mode);
    expect(loadRemoteControl()).toBe(mode);
  });

  it.each([
    ["", "empty"],
    ["   ", "blank"],
    ["ALL", "wrong case"],
    [" all ", "padded"],
    ["everything", "unknown mode"],
    ["1", "a boolean flag value"],
    ["true", "a boolean flag word"],
    ['{"mode":"all"}', "JSON"],
  ])("falls back to manual for %j (%s)", (stored) => {
    localStorage.setItem(KEY, stored);
    expect(loadRemoteControl()).toBe("manual");
  });

  it("falls back to manual when storage cannot be read", () => {
    saveRemoteControl("all");
    vi.spyOn(localStorage, "getItem").mockImplementation(() => {
      throw new Error("storage unavailable");
    });
    expect(loadRemoteControl()).toBe("manual");
  });

  it("keeps the default and stays quiet when writing fails", () => {
    vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("storage quota exceeded");
    });
    expect(() => saveRemoteControl("all")).not.toThrow();
    expect(loadRemoteControl()).toBe("manual");
  });

  it("tolerates unavailable storage", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(loadRemoteControl()).toBe("manual");
    expect(() => saveRemoteControl("all")).not.toThrow();
  });
});

describe("subscribeRemoteControl", () => {
  it("notifies listeners after the new mode is readable", () => {
    const listener = vi.fn(() => {
      // Switching to `all` has to be visible to whoever is reacting.
      expect(loadRemoteControl()).toBe("all");
    });
    const unsubscribe = subscribeRemoteControl(listener);
    try {
      saveRemoteControl("all");
      expect(listener).toHaveBeenCalledTimes(1);
    } finally {
      unsubscribe();
    }
  });

  it("carries the new mode on the event", () => {
    const seen: string[] = [];
    const listener = (event: Event) => {
      seen.push((event as CustomEvent<string>).detail);
    };
    window.addEventListener(REMOTE_CONTROL_CHANGE_EVENT, listener);
    try {
      saveRemoteControl("all");
      saveRemoteControl("manual");
      expect(seen).toEqual(["all", "manual"]);
    } finally {
      window.removeEventListener(REMOTE_CONTROL_CHANGE_EVENT, listener);
    }
  });

  it("stops notifying once unsubscribed", () => {
    const listener = vi.fn();
    subscribeRemoteControl(listener)();
    saveRemoteControl("all");
    expect(listener).not.toHaveBeenCalled();
  });

  it("still announces the change when the write fails", () => {
    // A listener that never fires would leave live sessions on the old mode.
    vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("storage quota exceeded");
    });
    const listener = vi.fn();
    const unsubscribe = subscribeRemoteControl(listener);
    try {
      expect(() => saveRemoteControl("all")).not.toThrow();
      expect(listener).toHaveBeenCalledTimes(1);
    } finally {
      unsubscribe();
    }
  });

  it("returns a no-op unsubscribe without a window", () => {
    vi.stubGlobal("window", undefined);
    expect(() => subscribeRemoteControl(() => {})()).not.toThrow();
  });
});
