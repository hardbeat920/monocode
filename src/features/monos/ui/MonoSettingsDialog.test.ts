// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  RUNTIME_MODE_LABEL,
  RUNTIME_MODES,
  type RuntimeMode,
} from "../../sessions/model/session";
import { MonoSettingsDialog } from "./MonoSettingsDialog";

vi.mock("../../sessions/ui/ModelPicker", () => ({
  ModelPicker: () => null,
  ModelSettingRows: () => null,
}));

const saveFile = vi.hoisted(() => vi.fn());
const loadFiles = vi.hoisted(() => vi.fn());

vi.mock("../model/monoFiles", async (original) => ({
  ...(await original<object>()),
  saveMonoFile: saveFile,
  loadMonoFiles: loadFiles,
}));

const loadedFiles = {
  id: "mono-1",
  dir: "/data/mono-1",
  soul: "# Soul",
  soulHash: "soul",
  memory: "",
  memoryHash: "memory",
  memoryPath: "/data/mono-1/MEMORY.md",
  topics: [],
};

let container: HTMLDivElement;
let root: Root;
const onRuntimeModeChange = vi.fn();
const onClose = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  loadFiles.mockResolvedValue(loadedFiles);
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

async function render(
  runtimeMode: RuntimeMode = "auto",
  busy = false,
  onReset?: () => Promise<void>,
) {
  await act(async () =>
    root.render(
      createElement(MonoSettingsDialog, {
        monoId: "mono-1",
        agent: { name: "Broski", mascot: "cat", color: "#9c9", projects: [] },
        status: busy ? "working" : "idle",
        cwd: "/home",
        harness: "codex",
        model: "codex:gpt-5.4",
        modelSettings: {},
        runtimeMode,
        busy,
        onModelChange: vi.fn(),
        onModelSettingsChange: vi.fn(),
        onRuntimeModeChange,
        onReset,
        onClose,
      }),
    ),
  );
}

function trigger() {
  return document.querySelector<HTMLButtonElement>(
    "[data-access-picker-trigger]",
  )!;
}

function dialog() {
  return document.querySelector<HTMLElement>("[data-mono-settings]")!;
}

function button(label: string, root: ParentNode = document) {
  return [...root.querySelectorAll<HTMLButtonElement>("button")].find(
    (entry) => entry.textContent === label,
  )!;
}

function escape() {
  act(() =>
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
}

it("opens on General and moves between its sections", async () => {
  await render();
  const nav = dialog().querySelector('nav[aria-label="Settings sections"]')!;
  expect([...nav.querySelectorAll("button")].map((b) => b.textContent)).toEqual(
    ["General", "Soul", "Memory"],
  );
  expect(button("General", nav).getAttribute("aria-current")).toBe("page");
  expect(dialog().querySelector('input[aria-label="Name"]')).not.toBeNull();
  expect(dialog().textContent).toContain("Permissions");
  await act(async () => button("Memory", nav).click());
  expect(button("Memory", nav).getAttribute("aria-current")).toBe("page");
  expect(dialog().querySelector("[data-mono-memory]")).not.toBeNull();
  await act(async () => button("Soul", nav).click());
  expect(dialog().querySelector("[data-mono-soul]")).not.toBeNull();
});

it("keeps the mascots and colors in a popover opened from its icon", async () => {
  await render();
  expect(document.querySelector('[aria-label="Mascot"]')).toBeNull();
  const face = dialog().querySelector<HTMLElement>(
    '[aria-label="Change mascot and color"]',
  )!;
  act(() => face.click());
  const popover = document.querySelector('[aria-label="Mascot and color"]')!;
  expect(popover.querySelector('[aria-label="Mascot"]')).not.toBeNull();
  expect(popover.querySelector('[aria-label="Color"]')).not.toBeNull();
  // Escape closes the popover first, then the dialog.
  escape();
  expect(document.querySelector('[aria-label="Mascot and color"]')).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  escape();
  expect(onClose).toHaveBeenCalledOnce();
});

function confirmButton(label: string) {
  return [
    ...document.querySelectorAll<HTMLButtonElement>(
      '[role="dialog"]:not(:has([data-mono-settings])) button',
    ),
  ].find((entry) => entry.textContent === label)!;
}

it("requires confirmation for reset and cancels without deleting anything", async () => {
  const reset = vi.fn(async () => {});
  await render("auto", false, reset);
  act(() => button("Reset", dialog()).click());
  const confirm = confirmButton("Cancel").closest('[role="dialog"]')!;
  expect(confirm.textContent).toContain("Reset Broski's conversation?");
  expect(confirm.textContent).toContain("soul, memory and habits will be kept");
  act(() => confirmButton("Cancel").click());
  expect(confirmButton("Cancel")).toBeUndefined();
  expect(reset).not.toHaveBeenCalled();
});

it("resets once after confirmation", async () => {
  const reset = vi.fn(async () => {});
  await render("auto", false, reset);
  act(() => button("Reset", dialog()).click());
  await act(async () => confirmButton("Reset conversation").click());
  expect(reset).toHaveBeenCalledOnce();
  expect(confirmButton("Cancel")).toBeUndefined();
});

it("lets the user choose every permission mode and reflects the saved choice", async () => {
  await render("auto");
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

it("keeps a conflicting soul draft open when closing", async () => {
  const { MonoFileConflict } = await import("../model/monoFiles");
  saveFile.mockRejectedValueOnce(new MonoFileConflict());
  await render();
  await act(async () => button("Soul", dialog()).click());
  const editor = document.querySelector<HTMLTextAreaElement>(
    '[aria-label="Broski soul"]',
  )!;
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    setter.call(editor, "# Changed soul");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  expect(saveFile).toHaveBeenCalledWith(
    "mono-1",
    "soul",
    "# Changed soul",
    "soul",
  );
  expect(onClose).not.toHaveBeenCalled();
  expect(editor.value).toBe("# Changed soul");
  expect(dialog().textContent).toContain("changed this while you were editing");
});

it("preserves edits made while loading the conflicting version", async () => {
  const { MonoFileConflict } = await import("../model/monoFiles");
  saveFile.mockRejectedValueOnce(new MonoFileConflict());
  await render();
  await act(async () => button("Soul", dialog()).click());
  const editor = document.querySelector<HTMLTextAreaElement>(
    '[aria-label="Broski soul"]',
  )!;
  const edit = (value: string) => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, value);
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  };
  await act(async () => edit("first draft"));
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  let resolve!: (value: typeof loadedFiles) => void;
  loadFiles.mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  act(() => button("Use theirs").click());
  await act(async () => edit("newer draft"));
  await act(async () =>
    resolve({ ...loadedFiles, soul: "theirs", soulHash: "new" }),
  );
  expect(editor.value).toBe("newer draft");
  expect(dialog().textContent).toContain("changed this while you were editing");
});

it("keeps a conflicting draft if the file changes again when keeping mine", async () => {
  const { MonoFileConflict } = await import("../model/monoFiles");
  saveFile
    .mockRejectedValueOnce(new MonoFileConflict())
    .mockRejectedValueOnce(new MonoFileConflict());
  await render();
  await act(async () => button("Soul", dialog()).click());
  const editor = document.querySelector<HTMLTextAreaElement>(
    '[aria-label="Broski soul"]',
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Draft");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  loadFiles.mockResolvedValueOnce({
    ...loadedFiles,
    soul: "# Their update",
    soulHash: "fresh",
  });
  await act(async () => button("Keep mine", dialog()).click());
  expect(saveFile).toHaveBeenLastCalledWith(
    "mono-1",
    "soul",
    "# Draft",
    "fresh",
  );
  expect(editor.value).toBe("# Draft");
  expect(dialog().textContent).toContain("changed this while you were editing");
});
