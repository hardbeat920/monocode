// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MAX_ATTACHMENTS } from "../model/attachments";
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

/** Wait for the async clipboard read and the attachments it adds. */
/** Every chip renders a `Remove <name>` control. */
function chipCount() {
  return container.querySelectorAll('[aria-label^="Remove "]').length;
}

function alert() {
  return container.querySelector('[role="alert"]')?.textContent ?? null;
}

/**
 * Wait for an outcome rather than a fixed delay: reading a clipboard image
 * crosses an IPC hop and a FileReader, so the chain needs real macrotasks to
 * finish. `vi.waitFor` cannot be used here because polling inside `act` starves
 * the FileReader timer.
 */
async function settleUntil(check: () => boolean, what: string) {
  for (let waited = 0; waited < 2000; waited += 10) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
    });
    if (check()) return;
  }
  throw new Error(`Timed out waiting for ${what}.`);
}

/** A paste that is expected to attach nothing still needs its chain flushed. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 10));
  });
}

async function waitForAttachments(count: number) {
  await settleUntil(() => chipCount() === count, `${count} attachment chip(s)`);
}

async function waitForAlert() {
  await settleUntil(() => alert() !== null, "an attachment error");
}

async function send() {
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
      .click(),
  );
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
  await waitForAttachments(1);
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

it("attaches a file copied in a file manager instead of inserting its URI", async () => {
  clipboardPaths = ["/home/dev/report.pdf"];
  const event = paste(render(), "file:///home/dev/report.pdf");
  await waitForAttachments(1);
  await send();

  // A file copy is not a screenshot, so the image read is never reached.
  expect(invoke).not.toHaveBeenCalledWith("clipboard_image");
  // The chip replaces the URI, so the draft must not also carry it.
  expect(event.defaultPrevented).toBe(true);
  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({
      name: "report.pdf",
      path: "/home/dev/report.pdf",
      mimeType: "application/pdf",
    }),
  ]);
});

it("keeps a file URI as draft text when the native read finds no copied path", async () => {
  clipboardPaths = [];
  const field = render("");
  const event = paste(field, "file:///home/dev/report.pdf");
  await settleUntil(
    () => field.value === "file:///home/dev/report.pdf",
    "the URI back in the draft",
  );

  // Nothing attachable, so the text the webview was denied is restored.
  expect(event.defaultPrevented).toBe(true);
  expect(chipCount()).toBe(0);
  expect(alert()).toBeNull();
});

it("leaves a whitespace-only paste to the webview", async () => {
  const event = paste(render(), "   ");
  await settle();

  // A space or a newline is text a person meant to paste, not a lost paste.
  expect(event.defaultPrevented).toBe(false);
  expect(invoke).not.toHaveBeenCalled();
});

it("attaches a copied file even when the webview sees no text at all", async () => {
  clipboardPaths = ["/home/dev/notes.md"];
  paste(render());
  await waitForAttachments(1);
  await send();

  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({ name: "notes.md", path: "/home/dev/notes.md" }),
  ]);
});

it("attaches a copied folder so its path reaches the agent", async () => {
  clipboardPaths = ["/home/dev/reports"];
  clipboardPathsAreDirs = true;
  paste(render(), "file:///home/dev/reports");
  await waitForAttachments(1);
  await send();

  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({
      name: "reports",
      path: "/home/dev/reports",
      mimeType: "inode/directory",
    }),
  ]);
});

it("reports a copy that holds nothing attachable", async () => {
  clipboardPaths = ["/home/dev/.DS_Store"];
  paste(render(), "file:///home/dev/.DS_Store");
  await waitForAlert();

  expect(alert()).toBe(
    "Nothing to attach from that path — the file may have been moved, renamed, or deleted.",
  );
});

it("attaches what fits and says how many copies it left out", async () => {
  clipboardPaths = Array.from(
    { length: MAX_ATTACHMENTS + 3 },
    (_, index) => `/home/dev/file-${index}.txt`,
  );
  paste(render(), "file:///home/dev/file-0.txt");
  await waitForAlert();

  expect(alert()).toBe(
    `Attached ${MAX_ATTACHMENTS} of ${MAX_ATTACHMENTS + 3} copied files. A turn carries up to ${MAX_ATTACHMENTS}.`,
  );
  await send();
  expect(submit.mock.calls[0][1]).toHaveLength(MAX_ATTACHMENTS);
});

it("fills the turn from later paths when an early one cannot be attached", async () => {
  clipboardPaths = [
    "/home/dev/.DS_Store",
    ...Array.from(
      { length: MAX_ATTACHMENTS },
      (_, index) => `/home/dev/file-${index}.txt`,
    ),
  ];
  paste(render(), "file:///home/dev/.DS_Store");
  await waitForAttachments(MAX_ATTACHMENTS);
  await send();

  // The .DS_Store is skipped, so the quota is filled from the paths behind it
  // rather than leaving a slot empty.
  expect(alert()).toBeNull();
  const attached = submit.mock.calls[0][1] as { name: string }[];
  expect(attached).toHaveLength(MAX_ATTACHMENTS);
  expect(attached.map((file) => file.name)).not.toContain(".DS_Store");
  expect(attached.at(-1)!.name).toBe(`file-${MAX_ATTACHMENTS - 1}.txt`);
});

it("leaves a text paste to the webview without reading the native clipboard", async () => {
  const event = paste(render(), "pasted words");
  await settle();

  expect(event.defaultPrevented).toBe(false);
  // A real text paste must not cost a clipboard read at all.
  expect(invoke).not.toHaveBeenCalled();
  expect(alert()).toBeNull();
  expect(submit).not.toHaveBeenCalled();
});

it("reports a clipboard it could not read instead of trying the image", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "clipboard_file_paths")
      throw "The clipboard could not be read on this system.";
    return [];
  });
  const event = paste(render(), "file:///home/dev/report.pdf");
  await waitForAlert();

  expect(alert()).toBe("The clipboard could not be read on this system.");
  // A failed read must not be mistaken for an empty clipboard.
  expect(invoke).not.toHaveBeenCalledWith("clipboard_image");
  // The URI is still withheld, so the error is what the user is left with.
  expect(event.defaultPrevented).toBe(true);
});

it("reports why a clipboard image could not be attached", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "clipboard_image")
      throw "Clipboard image is too large to attach (maximum 20 MB).";
    return [];
  });
  paste(render());
  await waitForAlert();

  expect(alert()).toBe(
    "Clipboard image is too large to attach (maximum 20 MB).",
  );
  expect(
    container.querySelector('[aria-label^="Open clipboard-image"]'),
  ).toBeNull();
});
