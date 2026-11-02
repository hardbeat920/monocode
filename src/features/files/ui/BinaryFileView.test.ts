// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BinaryFileView } from "./BinaryFileView";

const actions = vi.hoisted(() => ({
  copyFileToClipboard: vi.fn(async () => {}),
  copyText: vi.fn(async (_text: string) => {}),
  revealPath: vi.fn(async (_path: string) => {}),
  openPathWithDefaultApp: vi.fn(async () => {}),
  readBinaryFile: vi.fn(
    async () =>
      new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  ),
}));

vi.mock("../../../platform/tauri/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../platform/tauri/fs")>()),
  copyFileToClipboard: actions.copyFileToClipboard,
  openPathWithDefaultApp: actions.openPathWithDefaultApp,
  readBinaryFile: actions.readBinaryFile,
  revealPath: actions.revealPath,
}));

vi.mock("../../../platform/tauri/clipboard", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/clipboard")
  >()),
  copyText: actions.copyText,
}));

vi.mock("../model/fileWatch", () => ({
  watchFile: () => () => {},
}));

vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  IS_MAC: true,
  IS_WIN: false,
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:image-preview");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  vi.clearAllMocks();
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

async function renderViewer(
  path = "/repo/art/original image.png",
  visible = true,
  strict = false,
) {
  await act(async () => {
    const viewer = createElement(BinaryFileView, {
      path,
      cwd: "/repo",
      visible,
    });
    root.render(strict ? createElement(StrictMode, null, viewer) : viewer);
    await Promise.resolve();
  });
}

describe("BinaryFileView image copy", () => {
  it("replaces the webview image menu with an original-file copy action", async () => {
    await renderViewer();
    const image = container.querySelector("img")!;
    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 80,
      clientY: 60,
    });

    act(() => image.dispatchEvent(event));

    expect(event.defaultPrevented).toBe(true);
    const menu = document.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Image actions"]',
    )!;
    const copy = Array.from(
      menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((button) => button.textContent === "Copy Original File")!;

    await act(async () => copy.click());

    expect(actions.copyFileToClipboard).toHaveBeenCalledWith(
      "/repo/art/original image.png",
    );
  });

  it("copies the original file from the viewer footer", async () => {
    await renderViewer();

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Copy original file"]')!
        .click(),
    );

    expect(actions.copyFileToClipboard).toHaveBeenCalledWith(
      "/repo/art/original image.png",
    );
    expect(container.querySelector('[aria-label="Copied"]')).not.toBeNull();
  });
});

describe("BinaryFileView video playback", () => {
  it("renders a controlled player and shows its metadata", async () => {
    await renderViewer("/repo/clip.mp4");
    const video = container.querySelector("video")!;
    expect(video.src).toBe("blob:image-preview");
    expect(video.controls).toBe(true);
    expect(video.autoplay).toBe(false);
    expect(video.preload).toBe("metadata");
    expect(video.hasAttribute("playsinline")).toBe(true);
    expect(container.querySelector("img")).toBeNull();
    const blob = vi.mocked(URL.createObjectURL).mock.calls[0][0] as Blob;
    expect(blob.type).toBe("video/mp4");

    Object.defineProperties(video, {
      videoWidth: { value: 1920 },
      videoHeight: { value: 1080 },
      duration: { value: 70.8 },
    });
    act(() => video.dispatchEvent(new Event("loadedmetadata")));

    expect(container.textContent).toContain("1920 × 1080");
    expect(container.textContent).toContain("1:10");
    expect(container.textContent).toContain("8 B");
  });

  it("pauses a hidden tab without resuming when it becomes visible", async () => {
    await renderViewer("/repo/clip.mp4");
    const video = container.querySelector("video")!;
    const pause = vi.spyOn(video, "pause");
    const play = vi.spyOn(video, "play");

    await renderViewer("/repo/clip.mp4", false);
    expect(pause).toHaveBeenCalledOnce();
    await renderViewer("/repo/clip.mp4", true);
    expect(play).not.toHaveBeenCalled();

    vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(pause).toHaveBeenCalledTimes(2);
  });

  it("offers an external player and retries after a decoding error", async () => {
    await renderViewer("/repo/clip.mov");
    act(() =>
      container.querySelector("video")!.dispatchEvent(new Event("error")),
    );
    expect(container.querySelector("video")).toBeNull();
    expect(container.textContent).toContain("Couldn’t play clip.mov");
    expect(container.textContent).toContain("format or codec is unsupported");

    const buttons = [...container.querySelectorAll("button")];
    await act(async () =>
      buttons
        .find((button) => button.textContent === "Open externally")!
        .click(),
    );
    expect(actions.openPathWithDefaultApp).toHaveBeenCalledWith(
      "/repo/clip.mov",
    );
    vi.mocked(URL.createObjectURL).mockReturnValueOnce("blob:video-retry");
    await act(async () =>
      buttons.find((button) => button.textContent === "Retry")!.click(),
    );
    expect(container.querySelector("video")).not.toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:image-preview");
  });

  it("does not offer a local external player for remote videos", async () => {
    await renderViewer("remote://env/repo/clip.mp4");
    act(() =>
      container.querySelector("video")!.dispatchEvent(new Event("error")),
    );
    expect(container.textContent).not.toContain("Open externally");
  });

  it("releases the player and blob when closed", async () => {
    await renderViewer("/repo/clip.mp4");
    const video = container.querySelector("video")!;
    const pause = vi.spyOn(video, "pause");
    const load = vi.spyOn(video, "load");
    vi.useFakeTimers();
    act(() => root.render(null));
    expect(pause).toHaveBeenCalledOnce();
    act(() => vi.runAllTimers());
    vi.useRealTimers();
    expect(video.hasAttribute("src")).toBe(false);
    expect(load).toHaveBeenCalledOnce();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:image-preview");
  });

  it("keeps the source loading when StrictMode replays its effects", async () => {
    const load = vi
      .spyOn(HTMLMediaElement.prototype, "load")
      .mockImplementation(() => {});
    await renderViewer("/repo/clip.mp4", true, true);
    await act(() => new Promise((resolve) => setTimeout(resolve)));
    expect(container.querySelector("video")?.src).toBe("blob:image-preview");
    expect(load).not.toHaveBeenCalled();
  });

  it("shows file read errors and allows retry", async () => {
    actions.readBinaryFile.mockRejectedValueOnce(
      new Error("File is too large to preview (maximum 25 MB)."),
    );
    await renderViewer("/repo/clip.mp4");
    expect(container.textContent).toContain("maximum 25 MB");
    const retry = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Retry",
    )!;
    await act(async () => retry.click());
    expect(container.querySelector("video")).not.toBeNull();
  });
});

describe("BinaryFileView missing file card", () => {
  const missing = "/repo/shots/qa/full-page.jpg";

  afterEach(() => {
    actions.revealPath.mockImplementation(async () => {});
  });

  function readFails() {
    actions.readBinaryFile.mockRejectedValueOnce(
      new Error(`${missing}: No such file or directory (os error 2)`),
    );
  }

  async function renderMissing() {
    readFails();
    await renderViewer(missing);
    expect(container.textContent).toContain("Couldn’t open full-page.jpg");
  }

  function button(label: string) {
    return [...container.querySelectorAll("button")].find(
      (button) => button.textContent === label,
    )!;
  }

  function status() {
    return container.querySelector('[role="status"]')!.textContent;
  }

  it("reveals the closest folder that still exists", async () => {
    actions.revealPath.mockImplementation(async (path: string) => {
      if (path !== "/repo/shots") throw "No such file or directory";
    });
    await renderMissing();

    await act(async () => button("Reveal").click());

    expect(actions.revealPath.mock.calls.map(([path]) => path)).toEqual([
      missing,
      "/repo/shots/qa",
      "/repo/shots",
    ]);
    expect(status()).toBe(
      "The file is no longer there. Revealed shots instead.",
    );
  });

  it("says why Reveal failed instead of doing nothing", async () => {
    actions.revealPath.mockRejectedValue("Could not reveal in Finder.");
    await renderMissing();

    await act(async () => button("Reveal").click());

    expect(status()).toBe("Couldn’t reveal it: Could not reveal in Finder.");
  });

  it("does not climb remote paths", async () => {
    actions.revealPath.mockRejectedValue("No such file or directory");
    actions.readBinaryFile.mockRejectedValueOnce(new Error("gone"));
    await renderViewer("remote://env/repo/shot.png");

    await act(async () => button("Reveal").click());

    expect(actions.revealPath).toHaveBeenCalledOnce();
    expect(status()).toBe("Couldn’t reveal it: No such file or directory");
  });

  it("confirms a copied path and reports a failed copy", async () => {
    await renderMissing();

    await act(async () => button("Copy path").click());

    expect(actions.copyText).toHaveBeenCalledWith(missing);
    expect(button("Copied")).toBeDefined();

    actions.copyText.mockRejectedValueOnce(new Error("copy failed"));
    await act(async () => button("Copied").click());

    expect(status()).toBe("Couldn’t copy the path: copy failed");
  });

  it("says when Retry still cannot read the file", async () => {
    await renderMissing();
    expect(status()).toBe("");

    readFails();
    await act(async () => button("Retry").click());

    expect(actions.readBinaryFile).toHaveBeenCalledTimes(2);
    expect(status()).toBe("Still couldn’t open it.");
  });
});
