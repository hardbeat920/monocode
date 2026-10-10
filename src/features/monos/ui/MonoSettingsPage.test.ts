// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const saveMonoFile = vi.fn(async (..._args: unknown[]) => "next-hash");
vi.mock("../model/monoFiles", async (original) => ({
  ...(await original<object>()),
  saveMonoFile: (...args: unknown[]) => saveMonoFile(...args),
  editMonoFile: async (monoId: string, file: string, edit: (text: string) => string) =>
    saveMonoFile(monoId, file, edit(files.memory), files.memoryHash),
}));

const { MonoSettingsPage } = await import("./MonoSettingsPage");
const { MemoryPage } = await import("./MonoFilePages");
const { MonoPreferencesPage } = await import("./MonoPreferencesPage");

let root: Root;
let container: HTMLElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
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

const agent = { name: "Broski", mascot: "cat", color: "#9c9", project: "monocode" };
const files = {
  id: "a",
  dir: "/d",
  soul: "# Soul\n\n## Role\nThe resident agent for this project.",
  soulHash: "s",
  memory: "i am a cool pirate\n- 2026-10-04 · The user's name is Nick\n",
  memoryHash: "m",
  memoryPath: "/d/MEMORY.md",
  topics: [],
};

it("lists what lives behind each page, and opens it", () => {
  const onOpen = vi.fn();
  act(() =>
    root.render(
      createElement(MonoSettingsPage, {
        monoId: "mono-1",
        agent,
        onOpen,
        onBack: () => {},
      }),
    ),
  );
  const rows = [...container.querySelectorAll("nav button")].map((row) => row.textContent);
  expect(rows).toEqual([
    "SkillsAssign reusable skills or create ones for this Mono.",
    "SoulDefines who this bot is and the rules it follows. Always included in its context.",
    "HabitsRecurring tasks this bot runs on its own.",
    "MemoryFacts and preferences this bot remembers.",
  ]);
  act(() => (container.querySelectorAll("nav button")[3] as HTMLElement).click());
  expect(onOpen).toHaveBeenCalledWith("memory");
});

it("shows how many habits and facts it has once they load", () => {
  act(() =>
    root.render(
      createElement(MonoSettingsPage, {
        monoId: "mono-1",
        agent,
        onOpen: vi.fn(),
        counts: { habits: 3, memory: 12 },
      }),
    ),
  );
  const rows = [...container.querySelectorAll("nav button")].map((row) => row.textContent);
  expect(rows[1]).toMatch(/context\.$/);
  expect(rows[2]).toBe("HabitsRecurring tasks this bot runs on its own.3");
  expect(rows[3]).toBe("MemoryFacts and preferences this bot remembers.12");
});

function renderReset(onReset: () => Promise<void>) {
  act(() => root.render(createElement(MonoPreferencesPage, {
    monoId: "mono-1",
    agent,
    onBack: vi.fn(),
    onReset,
  })));
  return [...container.querySelectorAll("button")].find(
    (button) => button.textContent?.startsWith("Reset conversation"),
  )!;
}

function dialogButton(label: string) {
  return [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(
    (button) => button.textContent === label,
  )!;
}

it("requires confirmation for reset and cancels without deleting anything", () => {
  const reset = vi.fn(async () => {});
  const open = renderReset(reset);
  act(() => open.click());
  const dialog = document.querySelector('[role="dialog"]')!;
  expect(dialog.textContent).toContain("Reset Broski's conversation?");
  expect(dialog.textContent).toContain("All messages");
  expect(dialog.textContent).toContain("soul, memory, habits and skills will be kept");
  expect(reset).not.toHaveBeenCalled();
  act(() => dialogButton("Cancel").click());
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.activeElement).toBe(open);
  act(() => open.click());
  act(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(reset).not.toHaveBeenCalled();
});

it("resets once after confirmation and keeps the dialog while deletion is pending", async () => {
  let finish!: () => void;
  const reset = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
  const open = renderReset(reset);
  act(() => open.click());
  act(() => dialogButton("Reset conversation").click());
  expect(reset).toHaveBeenCalledOnce();
  expect(dialogButton("Resetting…").disabled).toBe(true);
  expect(dialogButton("Cancel").disabled).toBe(true);
  act(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  await act(async () => finish());
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(reset).toHaveBeenCalledOnce();
});

it("shows a failed reset and lets the user retry", async () => {
  const reset = vi.fn()
    .mockRejectedValueOnce(new Error("Storage unavailable"))
    .mockResolvedValueOnce(undefined);
  const open = renderReset(reset);
  act(() => open.click());
  await act(async () => dialogButton("Reset conversation").click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("Storage unavailable");
  expect(dialogButton("Reset conversation").disabled).toBe(false);
  await act(async () => dialogButton("Reset conversation").click());
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(reset).toHaveBeenCalledTimes(2);
});

it("shows memory as facts and forgets one without touching the rest", async () => {
  act(() =>
    root.render(
      createElement(MemoryPage, { monoId: "mono-1", files, onBack: () => {} }),
    ),
  );
  const facts = [...container.querySelectorAll("[data-memory-line]")].map((row) => row.textContent);
  expect(facts[0]).toBe("i am a cool pirate");
  expect(facts[1]).toContain("The user's name is Nick");
  await act(async () => (container.querySelector('[aria-label="Forget: i am a cool pirate"]') as HTMLElement).click());
  expect(saveMonoFile).toHaveBeenCalledWith(
    "mono-1",
    "memory",
    "- 2026-10-04 · The user's name is Nick\n",
    "m",
  );
});

it("adds a fact typed in the Memory page as a dated entry", async () => {
  saveMonoFile.mockClear();
  act(() =>
    root.render(
      createElement(MemoryPage, { monoId: "mono-1", files, onBack: () => {} }),
    ),
  );
  const add = container.querySelector('[aria-label="Add memory"]') as HTMLElement;
  act(() => add.click());
  const input = container.querySelector('[aria-label="New memory"]') as HTMLTextAreaElement;
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
    set.call(input, "Deploys go through Fly");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
  const [, , text, hash] = saveMonoFile.mock.calls[0] as unknown as string[];
  expect(text).toMatch(/- \d{4}-\d{2}-\d{2} · Deploys go through Fly\n$/);
  expect(text.startsWith(files.memory)).toBe(true);
  expect(hash).toBe("m");
});
