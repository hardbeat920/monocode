// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

const actions = vi.hoisted(() => ({
  copyText: vi.fn(async () => {}),
  openPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: actions.openPath,
}));

vi.mock("../lib/clipboard", () => ({
  copyText: actions.copyText,
}));

vi.mock("../lib/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/fs")>()),
  revealPath: actions.revealPath,
}));

let container: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof AgentMarkdown>;

function render() {
  act(() => root.render(createElement(AgentMarkdown, props)));
}

function openMenu(element: Element) {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    clientX: 100,
    clientY: 80,
  });
  act(() => element.dispatchEvent(event));
  return document.querySelector<HTMLElement>(
    '[role="menu"][aria-label="File link actions"]',
  );
}

async function pick(label: string) {
  const link = container.querySelector("a")!;
  const menu = openMenu(link)!;
  const item = Array.from(
    menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ).find((button) => button.textContent === label)!;
  await act(async () => item.click());
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  props = {
    text: "[Guide](/repo/docs/guide.md)",
    cwd: "/repo",
    onOpenFile: vi.fn(),
  };
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  render();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("AgentMarkdown file link context menu", () => {
  it("offers file-aware actions for a local Markdown link", () => {
    const menu = openMenu(container.querySelector("a")!);

    expect(menu).not.toBeNull();
    expect(menu!.textContent).toContain("Open in MonoCode");
    expect(menu!.textContent).toContain("Open in Default App");
    expect(menu!.textContent).toMatch(
      /Reveal in Finder|Reveal in File Explorer|Open Containing Folder/,
    );
    expect(menu!.textContent).toContain("Copy Path");
    expect(menu!.textContent).toContain("Copy Relative Path");
  });

  it("runs internal-open, external-open, reveal, and copy actions", async () => {
    await pick("Open in MonoCode");
    expect(props.onOpenFile).toHaveBeenCalledWith("/repo/docs/guide.md");

    await pick("Open in Default App");
    expect(actions.openPath).toHaveBeenCalledWith("/repo/docs/guide.md");

    const revealLabel = /Mac/.test(navigator.platform)
      ? "Reveal in Finder"
      : /Win/i.test(navigator.platform)
        ? "Reveal in File Explorer"
        : "Open Containing Folder";
    await pick(revealLabel);
    expect(actions.revealPath).toHaveBeenCalledWith("/repo/docs/guide.md");

    await pick("Copy Path");
    expect(actions.copyText).toHaveBeenLastCalledWith("/repo/docs/guide.md");

    await pick("Copy Relative Path");
    expect(actions.copyText).toHaveBeenLastCalledWith("docs/guide.md");
  });

  it("leaves external web links on the native context menu path", () => {
    props = { text: "[Website](https://example.com)", cwd: "/repo" };
    render();

    const menu = openMenu(container.querySelector("a")!);
    expect(menu).toBeNull();
  });

  it("also recognizes inline-code file references", () => {
    props = {
      text: "See `src/app.ts`.",
      cwd: "/repo",
      onOpenFile: vi.fn(),
    };
    render();

    const menu = openMenu(container.querySelector("code")!);
    expect(menu?.textContent).toContain("Open in MonoCode");
  });
});
