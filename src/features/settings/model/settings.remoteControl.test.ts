// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  REMOTE_CONTROL_DEFAULT,
  loadRemoteControl,
  saveRemoteControl,
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
