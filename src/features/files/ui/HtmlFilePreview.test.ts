// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { Storage } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FileEditor } from "./FileEditor";

const native = vi.hoisted(() => ({
  read: vi.fn(async () => "<h1>Rendered page</h1>"),
  open: vi.fn(async (_path: string) => {}),
}));
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke: vi.fn(async () => null),
}));
vi.mock("../../../platform/tauri/fs", async (original) => ({
  ...(await original<typeof import("../../../platform/tauri/fs")>()),
  readTextFile: native.read,
  openPathWithDefaultApp: native.open,
  statFiles: vi.fn(async () => []),
  gitDiffFiles: vi.fn(async () => ({ files: [] })),
  gitFileDiff: vi.fn(async () => ({ original: "", current: "", kind: "unstaged" })),
}));

describe("HTML and external file previews", () => {
  let root: Root;
  let container: HTMLDivElement;
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("localStorage", new Storage());
    native.read.mockReset().mockResolvedValue("<h1>Rendered page</h1>");
    native.open.mockClear();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });
  async function render(path = "D:/files/document.html", cwd = "D:/files") {
    await act(async () => {
      root.render(createElement(FileEditor, { path, cwd, active: true, onDirtyChange: () => {} }));
    });
  }
  async function click(label: string) {
    const button = [...container.querySelectorAll("button")].find((entry) => entry.textContent?.trim() === label);
    expect(button, `${label} control`).toBeDefined();
    await act(async () => button!.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  }
  it("renders HTML in an opaque sandbox without scripts", async () => {
    await render();
    const frame = container.querySelector("iframe");
    expect(frame).not.toBeNull();
    expect(frame!.getAttribute("sandbox")).toBe("");
    expect(frame!.srcdoc).toContain("<h1>Rendered page</h1>");
  });
  it("updates the preview from unsaved source edits", async () => {
    await render("D:/files/edit.html");
    await click("Source");
    const editor = EditorView.findFromDOM(container.querySelector(".cm-editor") as HTMLElement);
    expect(editor).not.toBeNull();
    await act(async () => {
      editor!.dispatch({ changes: { from: 0, to: editor!.state.doc.length, insert: "<h1>Edited page</h1>" } });
    });
    await click("Preview");
    expect(container.querySelector("iframe")!.srcdoc).toContain("Edited page");
  });
  it("opens a rejected local binary file in its default app", async () => {
    native.read.mockRejectedValue(new Error("Binary files cannot be edited."));
    await render("D:/files/archive.zip");
    await click("Open externally");
    expect(native.open).toHaveBeenCalledWith("D:/files/archive.zip");
  });
  it("does not offer a local opener for a remote file", async () => {
    await render("remote://machine/project/document.html", "remote://machine/project");
    expect([...container.querySelectorAll("button")].some((entry) => entry.textContent?.trim() === "Open externally")).toBe(false);
  });
});
