// @vitest-environment happy-dom

import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectBackgroundDialog } from "./ProjectBackgroundDialog";
import {
  loadProjectChatBackgroundSettings,
  saveProjectChatBackgroundSettings,
} from "../model/projectChatBackground";
import { NEW_THREAD_BACKGROUND_EFFECT_DEFAULT } from "../../settings/model/appearance";

vi.mock("../model/chatBackground", () => ({
  pickAndSaveProjectChatBackground: vi.fn(async () => "/backgrounds/mono.png"),
  clearProjectChatBackground: vi.fn(async () => {}),
  projectChatBackgroundSrc: (path: string) => path,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => path,
}));

let root: Root;
let container: HTMLDivElement;

function mockLocalStorage() {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
    clear: () => data.clear(),
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mockLocalStorage();
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => {
      throw new Error("Preview unavailable in test");
    }),
  );
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("project background dialog", () => {
  it("lets a Mono change its stored effect and visibility with one control", async () => {
    saveProjectChatBackgroundSettings("mono:a", {
      path: "/backgrounds/mono.png",
      emptyOpacity: 0.24,
      sessionOpacity: 0.24,
      scope: "all",
      effect: "gradient-blur",
    });

    await act(async () =>
      root.render(
        createElement(ProjectBackgroundDialog, {
          project: "mono:a",
          name: "MonoCrab",
          mono: true,
          onClose: vi.fn(),
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.textContent).toContain("Change image");
    expect(dialog.textContent).toContain("Remove background image");
    expect(dialog.textContent).toContain("Background effect");
    expect(dialog.querySelectorAll('input[type="range"]')).toHaveLength(1);
    expect(dialog.querySelector('[role="radiogroup"]')).toBeNull();
    expect(dialog.querySelector(".gradient-blur-background")).not.toBeNull();
    const visibility = dialog.querySelector<HTMLInputElement>(
      'input[type="range"]',
    )!;
    expect(visibility.value).toBe("24");
    expect(loadProjectChatBackgroundSettings("mono:a")?.effect).toBe(
      "gradient-blur",
    );
    await act(async () =>
      dialog
        .querySelector<HTMLButtonElement>(
          '[aria-label="Mono background effect: Haze"]',
        )!
        .click(),
    );
    const none = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ).find((option) => option.textContent === "None")!;
    await act(async () => none.click());
    expect(loadProjectChatBackgroundSettings("mono:a")?.effect).toBe("none");
    const props = Object.entries(visibility).find(([key]) =>
      key.startsWith("__reactProps"),
    )![1];
    await act(async () => props.onChange({ target: { value: "50" } }));
    expect(loadProjectChatBackgroundSettings("mono:a")).toMatchObject({
      emptyOpacity: 0.5,
      sessionOpacity: 0.5,
      scope: "all",
      effect: "none",
    });
    expect(dialog.querySelector("img")?.style.opacity).toBe("0.5");
  });

  it("defaults a new Mono background to Haze at 24% and never falls back to the global image", async () => {
    localStorage.setItem(
      "monocode.chatBackgroundPath",
      "/backgrounds/global.png",
    );
    localStorage.setItem("monocode.newThreadBackgroundEffect", "ascii");
    localStorage.setItem("monocode.chatBackgroundEmptyOpacity", "0.6");
    localStorage.setItem("monocode.chatBackgroundSessionOpacity", "0.5");
    localStorage.setItem("monocode.chatBackgroundScope", "empty");
    await act(async () =>
      root.render(
        createElement(ProjectBackgroundDialog, {
          project: "mono:a",
          name: "MonoCrab",
          mono: true,
          onClose: vi.fn(),
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.textContent).toContain("No background selected");
    expect(dialog.querySelector("img")).toBeNull();
    expect(
      dialog.querySelector<HTMLInputElement>('input[type="range"]')?.value,
    ).toBe("24");
    const choose = Array.from(
      dialog.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Choose image")!;
    await act(async () => choose.click());
    const defaults = {
      path: "/backgrounds/mono.png",
      emptyOpacity: 0.24,
      sessionOpacity: 0.24,
      effect: "gradient-blur",
      scope: "all",
    };
    expect(loadProjectChatBackgroundSettings("mono:a")).toEqual(defaults);
    expect(
      dialog.querySelector<HTMLElement>(".gradient-blur-background")?.style
        .opacity,
    ).toBe("0.24");
    await act(async () =>
      dialog
        .querySelector<HTMLButtonElement>(
          '[aria-label="Mono background effect: Haze"]',
        )!
        .click(),
    );
    const none = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ).find((option) => option.textContent === "None")!;
    await act(async () => none.click());
    const visibility = dialog.querySelector<HTMLInputElement>(
      'input[type="range"]',
    )!;
    const props = Object.entries(visibility).find(([key]) =>
      key.startsWith("__reactProps"),
    )![1];
    await act(async () => props.onChange({ target: { value: "40" } }));
    expect(loadProjectChatBackgroundSettings("mono:a")).toMatchObject({
      effect: "none",
      sessionOpacity: 0.4,
    });
    const remove = Array.from(
      dialog.querySelectorAll<HTMLButtonElement>("button"),
    ).find(
      (button) => button.textContent?.trim() === "Remove background image",
    )!;
    await act(async () => remove.click());
    expect(loadProjectChatBackgroundSettings("mono:a")).toBeNull();
    expect(dialog.textContent).toContain("No background selected");
    expect(dialog.querySelector("img")).toBeNull();
    expect(visibility.value).toBe("24");
    await act(async () => choose.click());
    expect(loadProjectChatBackgroundSettings("mono:a")).toEqual(defaults);
  });

  it("keeps a Mono's saved effect and visibility when changing its image", async () => {
    const saved = {
      path: "/backgrounds/old.png",
      emptyOpacity: 0.35,
      sessionOpacity: 0.4,
      scope: "all" as const,
      effect: "dither" as const,
    };
    saveProjectChatBackgroundSettings("mono:a", saved);
    await act(async () =>
      root.render(
        createElement(ProjectBackgroundDialog, {
          project: "mono:a",
          name: "MonoCrab",
          mono: true,
          onClose: vi.fn(),
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(
      dialog.querySelector('[aria-label="Mono background effect: Dither"]'),
    ).not.toBeNull();
    expect(
      dialog.querySelector<HTMLInputElement>('input[type="range"]')?.value,
    ).toBe("40");
    expect(loadProjectChatBackgroundSettings("mono:a")).toEqual(saved);
    const change = Array.from(
      dialog.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Change image")!;
    await act(async () => change.click());
    expect(loadProjectChatBackgroundSettings("mono:a")).toEqual({
      ...saved,
      path: "/backgrounds/mono.png",
    });
  });

  it("keeps project background defaults independent of Mono defaults", async () => {
    localStorage.setItem("monocode.chatBackgroundEmptyOpacity", "0.6");
    localStorage.setItem("monocode.chatBackgroundSessionOpacity", "0.5");
    localStorage.setItem("monocode.chatBackgroundScope", "empty");
    await act(async () =>
      root.render(
        createElement(ProjectBackgroundDialog, {
          project: "/work/alpha",
          name: "Alpha",
          onClose: vi.fn(),
        }),
      ),
    );
    const choose = Array.from(
      document.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Choose image")!;
    await act(async () => choose.click());
    expect(loadProjectChatBackgroundSettings("/work/alpha")).toMatchObject({
      emptyOpacity: 0.6,
      sessionOpacity: 0.5,
      scope: "empty",
      effect: NEW_THREAD_BACKGROUND_EFFECT_DEFAULT,
    });
  });

  it("changes the effect only for the selected project", async () => {
    saveProjectChatBackgroundSettings("/work/alpha", {
      path: "/backgrounds/alpha.png",
      emptyOpacity: 0.2,
      sessionOpacity: 0.3,
      scope: "all",
      effect: "none",
    });
    saveProjectChatBackgroundSettings("/work/beta", {
      path: "/backgrounds/beta.png",
      emptyOpacity: 0.2,
      sessionOpacity: 0.3,
      scope: "all",
      effect: "ascii",
    });

    await act(async () =>
      root.render(
        createElement(ProjectBackgroundDialog, {
          project: "/work/alpha",
          name: "Alpha",
          onClose: vi.fn(),
        }),
      ),
    );
    const dialog = document.querySelector<HTMLElement>(
      '[role="dialog"][aria-modal="true"]',
    )!;
    expect(dialog.className).toContain("max-h-[calc(100dvh-32px)]");
    expect(dialog.parentElement?.className).toContain("top-1/2");
    expect(dialog.parentElement?.className).toContain("-translate-y-1/2");

    const heading = Array.from(dialog.querySelectorAll("span")).find(
      (node) => node.textContent === "Background effect",
    )!;
    expect(heading.nextElementSibling?.textContent).toBe(
      "Shows the original artwork.",
    );
    expect(
      heading.parentElement?.nextElementSibling?.querySelector(
        '[aria-label="Project background effect: None"]',
      ),
    ).not.toBeNull();

    const trigger = document.querySelector<HTMLButtonElement>(
      '[aria-label="Project background effect: None"]',
    )!;
    expect(trigger.className).toContain("bg-transparent");
    expect(trigger.className).not.toContain("bg-background-base");
    await act(async () => trigger.click());
    const dither = Array.from(
      document.querySelectorAll<HTMLButtonElement>(
        '[role="listbox"][aria-label="Project background effect"] [role="option"]',
      ),
    ).find((option) => option.textContent?.includes("Dither"))!;
    await act(async () => dither.click());

    expect(trigger.getAttribute("aria-label")).toBe(
      "Project background effect: Dither",
    );
    expect(loadProjectChatBackgroundSettings("/work/alpha")?.effect).toBe(
      "dither",
    );
    expect(loadProjectChatBackgroundSettings("/work/beta")?.effect).toBe(
      "ascii",
    );
  });

  it("previews a project Haze background with its own image and visibility", async () => {
    saveProjectChatBackgroundSettings("/work/alpha", {
      path: "/backgrounds/alpha.png",
      emptyOpacity: 0.35,
      sessionOpacity: 0.55,
      scope: "empty",
      effect: "gradient-blur",
    });
    await act(async () =>
      root.render(
        createElement(ProjectBackgroundDialog, {
          project: "/work/alpha",
          name: "Alpha",
          onClose: vi.fn(),
        }),
      ),
    );

    const preview = document.querySelector<HTMLElement>(
      ".gradient-blur-background",
    )!;
    expect(preview.style.opacity).toBe("0.35");
    expect(preview.style.getPropertyValue("--chat-background-image")).toContain(
      "alpha.png",
    );
    expect(loadProjectChatBackgroundSettings("/work/alpha")?.effect).toBe(
      "gradient-blur",
    );
    expect(
      document.querySelector('[aria-label="Project background effect: Haze"]'),
    ).not.toBeNull();
  });
});
