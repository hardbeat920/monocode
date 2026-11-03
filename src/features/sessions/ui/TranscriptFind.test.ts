// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TranscriptFind } from "./TranscriptFind";
import type { Block } from "../model/session";

let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (fn: () => void) => setTimeout(fn, 0));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function find(query: string) {
  act(() =>
    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "f",
        ctrlKey: true,
        metaKey: true,
        bubbles: true,
      }),
    ),
  );
  const input = container.querySelector<HTMLInputElement>("input")!;
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, query);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function render(
  text: string,
  onSearch?: (query: string) => Promise<string[]>,
  onNavigate = vi.fn(() => true),
) {
  act(() =>
    root.render(
      createElement(TranscriptFind, {
        blocks: [{ id: "reply", role: "assistant", text } as Block],
        visible: true,
        focused: true,
        onSearch,
        onNavigate,
      }),
    ),
  );
}

it("searches the archive while streaming and retains results across block updates", async () => {
  const onSearch = vi.fn(async () => ["archived", "reply"]);
  const onNavigate = vi.fn(() => true);
  render("needle", onSearch, onNavigate);
  find("needle");
  for (let i = 0; i < 5; i++) {
    await act(async () => {
      vi.advanceTimersByTime(100);
    });
    render(`needle ${i}`, onSearch, onNavigate);
  }
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("needle");
  expect(container.textContent).toContain("1 of 2");
  render("needle, more output", onSearch, onNavigate);
  expect(container.textContent).toContain("1 of 2");
  expect(container.textContent).not.toContain("Searching");

  // Live results update without another archive read, including removal of a hit.
  render("different output", onSearch, onNavigate);
  expect(container.textContent).toContain("1 of 1");
  render("needle returns", onSearch, onNavigate);
  expect(container.textContent).toContain("1 of 2");
  expect(onSearch).toHaveBeenCalledTimes(1);
});

it("ignores an obsolete archive response after the query changes", async () => {
  let finishFirst!: (ids: string[]) => void;
  const onSearch = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise<string[]>((resolve) => {
          finishFirst = resolve;
        }),
    )
    .mockResolvedValueOnce(["second-hit"]);
  render("", onSearch);
  find("first");
  await act(async () => {
    vi.advanceTimersByTime(150);
  });
  find("second");
  await act(async () => {
    vi.advanceTimersByTime(150);
  });
  expect(container.textContent).toContain("1 of 1");
  await act(async () => finishFirst(["old-one", "old-two"]));
  expect(container.textContent).toContain("1 of 1");
});

it("preserves archive match order when navigation loads an older page", async () => {
  const onSearch = vi.fn(async () => ["first", "second", "reply"]);
  const onNavigate = vi.fn(() => true);
  render("needle", onSearch, onNavigate);
  find("needle");
  await act(async () => {
    vi.advanceTimersByTime(150);
  });
  act(() =>
    root.render(
      createElement(TranscriptFind, {
        blocks: [{ id: "first", role: "assistant", text: "needle" } as Block],
        visible: true,
        focused: true,
        onSearch,
        onNavigate,
      }),
    ),
  );
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Next match"]')!
      .click(),
  );
  await act(async () => {
    vi.advanceTimersByTime(1);
  });
  expect(onNavigate).toHaveBeenLastCalledWith("second", "needle");
  expect(container.textContent).toContain("2 of 3");
  expect(onSearch).toHaveBeenCalledTimes(1);
});

it("updates live-only search results as new output arrives", () => {
  render("no match");
  find("needle");
  expect(container.textContent).toContain("No results");
  render("needle");
  expect(container.textContent).toContain("1 of 1");
});
