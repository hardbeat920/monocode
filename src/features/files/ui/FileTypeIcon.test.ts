// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FileTypeIcon } from "./FileTypeIcon";

vi.mock("./missingFolderIcons.json", () => ({
  default: {
    "folder-features-open": '<svg data-icon="overlay-features-open"></svg>',
  },
}));

vi.mock("react-material-icon-theme", () => ({
  getFileIcon: ({ fileExtension }: { fileExtension?: string }) =>
    fileExtension ?? "",
  getFolderIcon: ({
    folderName,
    isOpen,
  }: {
    folderName: string;
    isOpen: boolean;
  }) => {
    const base = folderName === "src" ? "folder" : `folder-${folderName}`;
    return isOpen ? `${base}-open` : base;
  },
  // Like the package, returns nothing for icons it names but ships no art for.
  getIconSvg: (name: string) =>
    [
      "folder-features-open",
      "folder-gone",
      "folder-gone-open",
      "folder-half-open",
    ].includes(name)
      ? ""
      : `<svg data-icon="${name}"><path /></svg>`,
}));

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
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("FileTypeIcon DOM updates", () => {
  it("preserves unchanged SVGs through repeated parent updates, including hidden panels", async () => {
    const render = (tick: number) =>
      root.render(
        createElement(
          "div",
          { hidden: true, "data-tick": tick },
          Array.from({ length: 32 }, (_, key) =>
            createElement(FileTypeIcon, { key, name: "file.ts", isDir: false }),
          ),
        ),
      );
    await act(async () => render(0));
    const svgs = [...container.querySelectorAll("svg")];
    expect(svgs).toHaveLength(32);
    const writes = vi.spyOn(Element.prototype, "innerHTML", "set");

    for (let tick = 1; tick <= 20; tick++) act(() => render(tick));

    expect(writes.mock.calls.length).toBe(0);
    container.querySelectorAll("svg").forEach((svg, index) => {
      expect(svg).toBe(svgs[index]);
    });
  });

  it("updates dimensions and filenames without replacing an unchanged glyph", async () => {
    const render = (name: string, size: number) =>
      root.render(createElement(FileTypeIcon, { name, size, isDir: false }));
    await act(async () => render("first.ts", 16));
    const svg = container.querySelector("svg");
    const writes = vi.spyOn(Element.prototype, "innerHTML", "set");

    act(() => render("second.ts", 24));
    expect(container.querySelector("svg")).toBe(svg);
    expect((container.firstElementChild as HTMLElement).style.width).toBe(
      "24px",
    );
    expect(writes.mock.calls.length).toBe(0);

    act(() => render("second.rs", 24));
    expect(container.querySelector("svg")?.getAttribute("data-icon")).toBe(
      "rs",
    );
    expect(writes).toHaveBeenCalledTimes(1);
  });

  it("updates folder glyphs when their expansion state changes", async () => {
    const render = (isOpen: boolean) =>
      root.render(
        createElement(FileTypeIcon, { name: "src", isDir: true, isOpen }),
      );
    await act(async () => render(false));
    expect(container.querySelector("svg")?.getAttribute("data-icon")).toBe(
      "folder",
    );
    act(() => render(true));
    expect(container.querySelector("svg")?.getAttribute("data-icon")).toBe(
      "folder-open",
    );
  });

  it("fills open glyphs the package dropped from the upstream theme", async () => {
    await act(async () =>
      root.render(
        createElement(FileTypeIcon, {
          name: "features",
          isDir: true,
          isOpen: true,
        }),
      ),
    );
    expect(container.querySelector("svg")?.getAttribute("data-icon")).toBe(
      "overlay-features-open",
    );
  });

  it.each([
    ["half", true, "folder-half"],
    ["gone", false, "folder"],
    ["gone", true, "folder-open"],
  ])(
    "falls back when no art exists for %s (open: %s)",
    async (name, isOpen, icon) => {
      await act(async () =>
        root.render(createElement(FileTypeIcon, { name, isDir: true, isOpen })),
      );
      expect(container.querySelector("svg")?.getAttribute("data-icon")).toBe(
        icon,
      );
    },
  );
});
