import { beforeEach, describe, expect, it } from "vitest";
import {
  clearProjectChatBackgroundSetting,
  loadProjectChatBackground,
  saveProjectChatBackground,
} from "./projectChatBackground";

const KEY = "monocode:project-chat-backgrounds";

function mockBrowserStorage() {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => data.set(key, value),
      removeItem: (key: string) => data.delete(key),
      clear: () => data.clear(),
      key: (index: number) => [...data.keys()][index] ?? null,
      get length() {
        return data.size;
      },
    },
    configurable: true,
  });
  Object.defineProperty(globalThis, "window", {
    value: { dispatchEvent: () => true },
    configurable: true,
  });
}

describe("project chat background settings", () => {
  beforeEach(mockBrowserStorage);

  it("stores independent overrides for each project", () => {
    saveProjectChatBackground("/work/alpha", {
      path: "/backgrounds/alpha.webp",
      opacity: 0.22,
      scope: "empty",
    });
    saveProjectChatBackground("/work/beta", {
      path: "/backgrounds/beta.png",
      opacity: 0.48,
      scope: "all",
    });

    expect(loadProjectChatBackground("/work/alpha")).toEqual({
      path: "/backgrounds/alpha.webp",
      opacity: 0.22,
      scope: "empty",
    });
    expect(loadProjectChatBackground("/work/beta")).toEqual({
      path: "/backgrounds/beta.png",
      opacity: 0.48,
      scope: "all",
    });
  });

  it("clamps visibility to the supported range", () => {
    saveProjectChatBackground("/work/alpha", {
      path: "/backgrounds/alpha.webp",
      opacity: 1,
      scope: "all",
    });
    saveProjectChatBackground("/work/beta", {
      path: "/backgrounds/beta.webp",
      opacity: 0,
      scope: "all",
    });

    expect(loadProjectChatBackground("/work/alpha")?.opacity).toBe(0.65);
    expect(loadProjectChatBackground("/work/beta")?.opacity).toBe(0.05);
  });

  it("falls back safely when stored project data is malformed", () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({
        "/work/alpha": {
          path: "/backgrounds/alpha.webp",
          opacity: "bright",
          scope: "transcript",
        },
      }),
    );

    expect(loadProjectChatBackground("/work/alpha")).toEqual({
      path: "/backgrounds/alpha.webp",
      opacity: 0.24,
      scope: "all",
    });
  });

  it("clears one project without changing the others", () => {
    saveProjectChatBackground("/work/alpha", {
      path: "/backgrounds/alpha.webp",
      opacity: 0.2,
      scope: "empty",
    });
    saveProjectChatBackground("/work/beta", {
      path: "/backgrounds/beta.webp",
      opacity: 0.3,
      scope: "all",
    });

    clearProjectChatBackgroundSetting("/work/alpha");

    expect(loadProjectChatBackground("/work/alpha")).toBeNull();
    expect(loadProjectChatBackground("/work/beta")?.path).toBe(
      "/backgrounds/beta.webp",
    );
  });
});
