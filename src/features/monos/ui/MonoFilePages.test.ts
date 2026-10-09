// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
import { MemoryPage } from "./MonoFilePages";

const initialMemory = "First fact\n- 2026-10-04 · Second fact\n";
let memory: string;
let version: number;
let beforeSave: (() => void) | undefined;
let saveFailure: string | undefined;
let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  memory = initialMemory;
  version = 0;
  beforeSave = undefined;
  saveFailure = undefined;
  invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === "mono_read") return { text: memory, hash: String(version) };
    if (command !== "mono_save")
      throw new Error(`Unexpected command: ${command}`);
    beforeSave?.();
    if (saveFailure) throw saveFailure;
    if (args.expectedHash !== String(version)) throw "conflict";
    memory = args.text;
    return String(++version);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function render(text = initialMemory) {
  act(() =>
    root.render(
      createElement(MemoryPage, {
        monoId: "mono",
        files: {
          id: "mono",
          dir: "/data/mono",
          soul: "",
          soulHash: "",
          memory: text,
          memoryHash: String(version),
          memoryPath: "/data/mono/MEMORY.md",
          topics: [],
        },
        onBack: () => {},
      }),
    ),
  );
}

function typeFact(label: string, text: string) {
  const input = container.querySelector<HTMLTextAreaElement>(
    `[aria-label="${label}"]`,
  )!;
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return input;
}

async function enter(input: HTMLTextAreaElement) {
  await act(async () => {
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  });
}

function add() {
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Add memory"]')!
      .click(),
  );
}

it("reapplies additions after a conflict and retains rapid additions before a reload", async () => {
  render();
  add();
  beforeSave = () => {
    beforeSave = undefined;
    memory += "Agent learned this\n";
    version++;
  };
  await enter(typeFact("New memory", "User learned this"));
  expect(memory).toContain("Agent learned this");
  expect(memory).toContain("User learned this");
  expect(container.querySelector<HTMLTextAreaElement>("textarea")!.value).toBe(
    "",
  );
  await enter(typeFact("New memory", "Another user fact"));
  expect(memory).toContain(initialMemory);
  expect(memory).toContain("Agent learned this");
  expect(memory).toContain("User learned this");
  expect(memory).toContain("Another user fact");
});

it("forgets the selected fact after another writer inserts a row", async () => {
  render();
  memory = `Inserted by agent\n${memory}`;
  version++;
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Forget: First fact"]')!
      .click(),
  );
  expect(memory).toBe("Inserted by agent\n- 2026-10-04 · Second fact\n");
});

it("keeps an editing draft on reload and edits its original fact after rows move", async () => {
  render();
  act(() =>
    container
      .querySelector<HTMLButtonElement>("[data-memory-line='0'] button")!
      .click(),
  );
  typeFact("Edit memory", "Updated first fact");
  memory = `Inserted by agent\n${memory}`;
  version++;
  render(memory);
  const input = container.querySelector<HTMLTextAreaElement>(
    '[aria-label="Edit memory"]',
  )!;
  expect(input.value).toBe("Updated first fact");
  await enter(input);
  expect(memory).toBe(
    "Inserted by agent\nUpdated first fact\n- 2026-10-04 · Second fact\n",
  );
});

it("shows repeated conflicts, preserves the draft, and allows a successful retry", async () => {
  render();
  add();
  saveFailure = "conflict";
  const input = typeFact("New memory", "Keep my draft");
  await enter(input);
  expect(
    invoke.mock.calls.filter(([command]) => command === "mono_save"),
  ).toHaveLength(3);
  expect(container.querySelector('[role="alert"]')!.textContent).toContain(
    "not saved",
  );
  expect(input.value).toBe("Keep my draft");
  expect(memory).toBe(initialMemory);
  saveFailure = undefined;
  await enter(input);
  expect(memory).toContain("Keep my draft");
  expect(input.value).toBe("");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});

it("keeps a draft and reports when its original fact changed during editing", async () => {
  render();
  act(() =>
    container
      .querySelector<HTMLButtonElement>("[data-memory-line='0'] button")!
      .click(),
  );
  typeFact("Edit memory", "My revision");
  memory = "Agent revised first fact\n- 2026-10-04 · Second fact\n";
  version++;
  render(memory);
  const input = container.querySelector<HTMLTextAreaElement>(
    '[aria-label="Edit memory"]',
  )!;
  expect(input.value).toBe("My revision");
  await enter(input);
  expect(container.querySelector('[role="alert"]')!.textContent).toContain(
    "memory changed",
  );
  expect(input.value).toBe("My revision");
  expect(memory).toBe("Agent revised first fact\n- 2026-10-04 · Second fact\n");
  expect(invoke.mock.calls.some(([command]) => command === "mono_save")).toBe(
    false,
  );
});

it("keeps a newly selected editor when an earlier blurred editor finishes saving", async () => {
  render();
  let finishSave!: () => void;
  const native = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command, args) => {
    if (command === "mono_save")
      await new Promise<void>((resolve) => {
        finishSave = resolve;
      });
    return native(command, args);
  });
  act(() =>
    container
      .querySelector<HTMLButtonElement>("[data-memory-line='0'] button")!
      .click(),
  );
  const input = typeFact("Edit memory", "Updated first fact");
  await act(async () =>
    input.dispatchEvent(new FocusEvent("focusout", { bubbles: true })),
  );
  act(() =>
    container
      .querySelector<HTMLButtonElement>("[data-memory-line='1'] button")!
      .click(),
  );
  typeFact("Edit memory", "Second draft");
  await act(async () => finishSave());
  expect(memory).toContain("Updated first fact");
  expect(
    container.querySelector<HTMLTextAreaElement>('[aria-label="Edit memory"]')!
      .value,
  ).toBe("Second draft");
});

it("closes an add field blurred during a pending save without saving twice", async () => {
  render();
  add();
  let finishSave!: () => void;
  const native = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (command, args) => {
    if (command === "mono_save")
      await new Promise<void>((resolve) => {
        finishSave = resolve;
      });
    return native(command, args);
  });
  const input = typeFact("New memory", "New fact");
  await enter(input);
  act(() => input.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  await act(async () => finishSave());
  expect(memory).toContain("New fact");
  expect(container.querySelector("textarea")).toBeNull();
  expect(
    invoke.mock.calls.filter(([command]) => command === "mono_save"),
  ).toHaveLength(1);
});

it("shows storage failures and retains the unsaved text", async () => {
  render();
  add();
  saveFailure = "Storage unavailable";
  const input = typeFact("New memory", "Unsaved fact");
  await enter(input);
  expect(container.querySelector('[role="alert"]')!.textContent).toBe(
    "Storage unavailable",
  );
  expect(input.value).toBe("Unsaved fact");
  expect(
    invoke.mock.calls.filter(([command]) => command === "mono_save"),
  ).toHaveLength(1);
});
