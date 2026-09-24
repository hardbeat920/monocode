// @vitest-environment happy-dom

import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectBackgroundDialog } from "./ProjectBackgroundDialog";
import {
  loadProjectChatBackgroundSettings,
  saveProjectChatBackgroundSettings,
} from "../model/projectChatBackground";

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
});
