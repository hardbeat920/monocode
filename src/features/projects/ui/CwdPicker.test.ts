// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CwdPicker } from "./CwdPicker";

let container: HTMLDivElement;
let root: Root;

function render(props: Partial<ComponentProps<typeof CwdPicker>>) {
  act(() =>
    root.render(
      createElement(CwdPicker, {
        cwd: "~",
        recents: [],
        onCwdChange: vi.fn(),
        ...props,
      }),
    ),
  );
}

function trigger(): HTMLButtonElement {
  return container.querySelector("button")!;
}

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

describe("CwdPicker trigger", () => {
  it("names what it does while there is no project", () => {
    render({ emptyLabel: "Move to project" });

    expect(trigger().textContent).toBe("Move to project");
    expect(trigger().getAttribute("aria-label")).toBe("Move to project");
  });

  it("shows the path without an empty label", () => {
    render({});

    expect(trigger().textContent).toBe("~");
  });

  it("shows the project once there is one", () => {
    render({ cwd: "/Users/me/app", emptyLabel: "Move to project" });

    expect(trigger().textContent).not.toContain("Move to project");
    expect(trigger().textContent).toContain("app");
  });
});
