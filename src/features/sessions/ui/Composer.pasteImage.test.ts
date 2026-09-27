// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Composer } from "./Composer";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async () => () => undefined,
  }),
}));

const PNG_BYTES = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
]);

let container: HTMLDivElement;
let root: Root;
let submit: ReturnType<typeof vi.fn>;
/** What a file manager would have put on the native clipboard. */
let clipboardPaths: string[];
/** Directories are reported by inspect_paths but cannot be attached. */
let clipboardPathsAreDirs: boolean;

function render(text = "") {
  act(() => {
    root.render(
      createElement(Composer, {
        focused: false,
        harness: "codex",
        model: "",
        runtimeMode: "supervised",
        executionCwd: "/repo",
        initialDraft: text,
        hideTopBar: true,
        onFocus: vi.fn(),
        onCwdChange: vi.fn(),
        onModelChange: vi.fn(),
        onRuntimeModeChange: vi.fn(),
        onSubmit: submit,
      }),
    );
  });
  return container.querySelector("textarea")!;
}

/** A paste the webview reports as text only, the way it does for copies. */
function paste(target: HTMLTextAreaElement, text = "") {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: {
      getData: (type: string) => (type === "text/plain" ? text : ""),
      files: [],
      items: [],
    },
  });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
}

async function send() {
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
      .click(),
  );
}

function alert() {
  return container.querySelector('[role="alert"]')?.textContent ?? null;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  submit = vi.fn();
  clipboardPaths = [];
  clipboardPathsAreDirs = false;
  invoke.mockReset();
  invoke.mockImplementation(
    async (command: string, args?: { paths?: string[] }) => {
      if (command === "clipboard_file_paths") return clipboardPaths;
      if (command === "clipboard_image") return PNG_BYTES.buffer;
      if (command === "inspect_paths")
        return (args?.paths ?? []).map((path) => ({
          path,
          name: path.split("/").pop() ?? path,
          size: 4096,
          isDir: clipboardPathsAreDirs,
        }));
      return [];
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("attaches an image the paste event reports as neither file nor text", async () => {
  const event = paste(render());
  await settle();
  await send();

  expect(event.defaultPrevented).toBe(true);
  expect(invoke).toHaveBeenCalledWith("clipboard_image");
  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({
      name: "clipboard-image.png",
      mimeType: "image/png",
      kind: "image",
    }),
  ]);
});

it("attaches a file copied in a file manager, leaving its text to the webview", async () => {
  clipboardPaths = ["/home/dev/report.pdf"];
  const event = paste(render(), "file:///home/dev/report.pdf");
  await settle();
  await send();

  // A file copy is not a screenshot, so the image read is never reached.
  expect(invoke).not.toHaveBeenCalledWith("clipboard_image");
  expect(event.defaultPrevented).toBe(false);
  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({
      name: "report.pdf",
      path: "/home/dev/report.pdf",
      mimeType: "application/pdf",
    }),
  ]);
});

it("attaches a copied file even when the webview sees no text at all", async () => {
  clipboardPaths = ["/home/dev/notes.md"];
  paste(render());
  await settle();
  await send();

  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({ name: "notes.md", path: "/home/dev/notes.md" }),
  ]);
});

it("reports a copy that has no attachable file in it", async () => {
  clipboardPaths = ["/home/dev/reports"];
  clipboardPathsAreDirs = true;
  paste(render(), "reports");
  await settle();

  expect(alert()).toBe(
    "Nothing to attach in that path. Only files can be attached.",
  );
});

it("leaves a text paste to the webview instead of reading image bytes", async () => {
  const event = paste(render(), "pasted words");
  await settle();

  expect(event.defaultPrevented).toBe(false);
  expect(invoke).not.toHaveBeenCalledWith("clipboard_image");
  expect(alert()).toBeNull();
  expect(submit).not.toHaveBeenCalled();
});

it("reports why a clipboard image could not be attached", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "clipboard_image")
      throw "Clipboard image is too large to attach (maximum 20 MB).";
    return [];
  });
  paste(render());
  await settle();

  expect(alert()).toBe(
    "Clipboard image is too large to attach (maximum 20 MB).",
  );
  expect(
    container.querySelector('[aria-label^="Open clipboard-image"]'),
  ).toBeNull();
});
