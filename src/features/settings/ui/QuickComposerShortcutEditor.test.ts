// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { SettingsView } from "./SettingsView";

vi.mock("../../../platform/tauri/platform", () => ({
  IS_MAC: true,
  IS_WIN: false,
  HAS_NATIVE_GLASS: true,
  MOD: "⌘",
  ALT: "⌥",
  SHIFT: "⇧",
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));

let container: HTMLDivElement;
let root: Root;
const data = new Map<string, string>();

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  data.clear();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  });
  vi.mocked(invoke).mockClear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

async function render() {
  await act(async () =>
    root.render(
      createElement(SettingsView, {
        section: "keybindings",
        cwd: "/repo",
        sessions: [],
        onClose: vi.fn(),
        onSelectSection: vi.fn(),
        onOpenSession: vi.fn(),
        onArchiveSession: vi.fn(),
        onDeleteSession: vi.fn(),
        onOpenWhatsNew: vi.fn(),
      }),
    ),
  );
}

it("records a global shortcut, persists it, and restores the default", async () => {
  await render();
  const input = container.querySelector<HTMLInputElement>(
    '[aria-label="Change quick composer shortcut"]',
  )!;
  expect(input.value).toBe("⌘⇧Space");
  await act(async () => input.click());
  expect(input.value).toBe("Record…");
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "MetaLeft",
        key: "Meta",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(input.value).toBe("⌘");
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "KeyK",
        key: "k",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(invoke).toHaveBeenCalledWith("quick_composer_set_enabled", {
    enabled: true,
    shortcut: "Command+KeyK",
  });
  expect(data.get("monocode.quickComposerShortcut")).toBe("Command+KeyK");
  expect(input.value).toBe("⌘K");

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Reset quick composer shortcut"]',
      )!
      .click(),
  );
  expect(data.get("monocode.quickComposerShortcut")).toBe(
    "Command+Shift+Space",
  );
  expect(input.value).toBe("⌘⇧Space");
});

it("keeps the previous shortcut when native registration fails", async () => {
  await render();
  vi.mocked(invoke).mockRejectedValueOnce("Shortcut is in use");
  const input = container.querySelector<HTMLInputElement>(
    '[aria-label="Change quick composer shortcut"]',
  )!;
  await act(async () => input.click());
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "KeyK",
        key: "k",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(input.value).toBe("⌘⇧Space");
  expect(data.has("monocode.quickComposerShortcut")).toBe(false);
  expect(container.textContent).toContain("Shortcut is in use");
});

it("accepts Control plus one key", async () => {
  await render();
  const input = container.querySelector<HTMLInputElement>(
    '[aria-label="Change quick composer shortcut"]',
  )!;
  await act(async () => input.click());
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "ControlLeft",
        key: "Control",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(input.value).toBe("⌃");
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "KeyY",
        key: "y",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(invoke).toHaveBeenCalledWith("quick_composer_set_enabled", {
    enabled: true,
    shortcut: "Control+KeyY",
  });
  expect(input.value).toBe("⌃Y");
});

it("shows pressed keys without an error and Escape cancels recording", async () => {
  await render();
  const input = container.querySelector<HTMLInputElement>(
    '[aria-label="Change quick composer shortcut"]',
  )!;
  await act(async () => input.click());
  expect(input.value).toBe("Record…");
  expect(container.textContent).toContain("Del disables · Esc cancels");
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "KeyK",
        key: "k",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(input.value).toBe("K");
  expect(container.textContent).not.toContain("Press Command");
  await act(async () =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        code: "Escape",
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  expect(input.value).toBe("⌘⇧Space");
  expect(container.textContent).not.toContain("Del disables · Esc cancels");
  expect(invoke).not.toHaveBeenCalledWith(
    "quick_composer_set_enabled",
    expect.anything(),
  );
});
