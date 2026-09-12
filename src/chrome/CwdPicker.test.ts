// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CwdPicker } from "./CwdPicker";

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

const recents = Array.from({ length: 30 }, (_, index) => ({
  path: `/Users/me/code/project-${index}`,
  openedAt: 30 - index,
}));

function menuItems(label: string): HTMLButtonElement[] {
  const menu = document.querySelector(`[role="menu"][aria-label="${label}"]`);
  return [
    ...(menu?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? []),
  ];
}

describe("cwd picker", () => {
  it("keeps a long project list to five rows and scrolls the rest", () => {
    const onCwdChange = vi.fn();
    act(() =>
      root.render(
        createElement(CwdPicker, {
          cwd: recents[0].path,
          recents,
          pill: true,
          onCwdChange,
        }),
      ),
    );
    act(() => container.querySelector("button")!.click());

    const rows = menuItems("Project picker");
    // The current project is left out, then five recents and "More Projects".
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("project-1"),
      expect.stringContaining("project-2"),
      expect.stringContaining("project-3"),
      expect.stringContaining("project-4"),
      expect.stringContaining("project-5"),
      "More Projects",
    ]);

    act(() => {
      rows[5].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    });
    const submenu = document.querySelector<HTMLElement>(
      '[role="menu"][aria-label="More projects"]',
    )!;
    expect(submenu.className).toContain("overflow-y-auto");
    const more = menuItems("More projects");
    expect(more).toHaveLength(24);

    act(() => more[23].click());
    expect(onCwdChange).toHaveBeenCalledWith("/Users/me/code/project-29");
  });
});
