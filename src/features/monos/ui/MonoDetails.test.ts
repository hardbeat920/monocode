// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  RUNTIME_MODE_LABEL,
  RUNTIME_MODES,
  type RuntimeMode,
} from "../../sessions/model/session";
import { MonoDetails } from "./MonoDetails";

vi.mock("../../sessions/ui/ModelPicker", () => ({
  ModelPicker: () => null,
  ModelSettingRows: () => null,
}));

vi.mock("../model/monoFiles", async (original) => ({
  ...(await original<object>()),
  loadMonoFiles: async () => ({
    id: "mono-1",
    dir: "/data/mono-1",
    soul: "# Soul",
    soulHash: "soul",
    memory: "",
    memoryHash: "memory",
    memoryPath: "/data/mono-1/MEMORY.md",
    topics: [],
  }),
}));

vi.mock("../model/monoHabits", async (original) => ({
  ...(await original<object>()),
  loadHabits: async () => [],
}));

let container: HTMLDivElement;
let root: Root;
const onRuntimeModeChange = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(runtimeMode: RuntimeMode, busy = false) {
  await act(async () =>
    root.render(
      createElement(MonoDetails, {
        open: true,
        monoId: "mono-1",
        cwd: "/home",
        agent: { name: "Broski", mascot: "cat", color: "#9c9", projects: [] },
        state: { status: busy ? "working" : "idle" },
        harness: "codex",
        model: "codex:gpt-5.4",
        modelSettings: {},
        runtimeMode,
        busy,
        onModelChange: vi.fn(),
        onModelSettingsChange: vi.fn(),
        onRuntimeModeChange,
        onClose: vi.fn(),
      }),
    ),
  );
}

function trigger() {
  return container.querySelector<HTMLButtonElement>(
    "[data-access-picker-trigger]",
  )!;
}

it("lets the user choose every permission mode from Details and reflects the saved choice", async () => {
  await render("auto");
  expect(container.querySelector("dl")?.textContent).toContain("Permissions");
  expect(trigger().textContent).toBe("Auto");

  for (const mode of RUNTIME_MODES) {
    act(() => trigger().click());
    const options = document.querySelectorAll<HTMLButtonElement>(
      '[data-access-picker] [role="option"]',
    );
    expect(options).toHaveLength(RUNTIME_MODES.length);
    act(() => options[RUNTIME_MODES.indexOf(mode)].click());
    expect(onRuntimeModeChange).toHaveBeenLastCalledWith(mode);
    expect(document.querySelector("[data-access-picker]")).toBeNull();
    await render(mode);
    expect(trigger().textContent).toBe(RUNTIME_MODE_LABEL[mode]);
  }
});

it("explains when permissions take effect while the Mono is working", async () => {
  await render("auto", true);
  expect(trigger().title).toContain("Changes apply to the next turn.");
  act(() => trigger().click());
  expect(document.querySelector("[data-access-picker]")?.textContent).toContain(
    "Access changes apply to the next turn. Stop and resend to apply them now.",
  );
});
