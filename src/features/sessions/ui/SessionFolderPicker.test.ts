// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionFolderPicker } from "./SessionFolderPicker";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("session folder picker", () => {
  it("chooses an existing sidebar folder", () => {
    const onPick = vi.fn();
    act(() =>
      root.render(
        createElement(SessionFolderPicker, {
          folders: [
            {
              id: "work",
              name: "Work",
              sessionIds: ["a", "b"],
              collapsed: false,
            },
          ],
          onPick,
          onDismiss: vi.fn(),
        }),
      ),
    );
    const option =
      container.querySelector<HTMLButtonElement>('[role="option"]')!;
    expect(option.textContent).toContain("Work");
    expect(option.textContent).toContain("2");
    act(() => option.click());
    expect(onPick).toHaveBeenCalledWith({
      kind: "existing",
      folderId: "work",
    });
  });

  it("creates a named sidebar folder", () => {
    const onPick = vi.fn();
    act(() =>
      root.render(
        createElement(SessionFolderPicker, {
          folders: [],
          onPick,
          onDismiss: vi.fn(),
        }),
      ),
    );
    type(container.querySelector("input")!, "Launch work");
    const option =
      container.querySelector<HTMLButtonElement>('[role="option"]')!;
    expect(option.textContent).toContain("Create “Launch work”");
    act(() => option.click());
    expect(onPick).toHaveBeenCalledWith({
      kind: "new",
      name: "Launch work",
    });
  });
});
