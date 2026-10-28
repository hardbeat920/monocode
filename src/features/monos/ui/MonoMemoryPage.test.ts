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

const { MemoryPage } = await import("./MonoFilePages");

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
