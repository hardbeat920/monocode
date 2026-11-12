// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { downloadFile } from "../../../platform/tauri/download";
import { readBinaryFile } from "../../../platform/tauri/fs";
import { fetchInboxMedia } from "../../inbox/model/inboxMedia";
import { InboxMedia } from "../../inbox/ui/InboxMedia";
import { LocalMedia } from "./LocalMedia";

vi.mock("../../../platform/tauri/download", () => ({ downloadFile: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset:${path}`,
}));
vi.mock("../../../platform/tauri/fs", () => ({
  readBinaryFile: vi.fn(),
  REMOTE_PATH_PREFIX: "remote://",
  basename: (path: string) => path.split("/").pop(),
}));
vi.mock("../../inbox/model/inboxMedia", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../inbox/model/inboxMedia")>()),
  fetchInboxMedia: vi.fn(),
}));

let root: Root;
let container: HTMLDivElement;
const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("IntersectionObserver", undefined);
  vi.mocked(downloadFile).mockReset().mockResolvedValue(undefined);
  vi.mocked(readBinaryFile).mockReset().mockResolvedValue(png);
  vi.mocked(fetchInboxMedia).mockReset().mockResolvedValue(png);
  vi.mocked(invoke).mockReset().mockResolvedValue("/video.mov");
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:preview");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
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

it.each([
  ["image.png", png],
  ["image.jpg", new Uint8Array([0xff, 0xd8, 0xff])],
  [
    "video.mov",
    new Uint8Array([
      0, 0, 0, 12, 0x66, 0x74, 0x79, 0x70, 0x71, 0x74, 0x20, 0x20,
    ]),
  ],
  ["video.webm", new Uint8Array([0x1a, 0x45, 0xdf, 0xa3])],
])(
  "downloads inbox media as %s using the already loaded blob",
  async (name, bytes) => {
    vi.mocked(fetchInboxMedia).mockResolvedValue(bytes);
    await act(async () =>
      root.render(
        createElement(InboxMedia, {
          src: "https://github.com/user-attachments/example",
        }),
      ),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(`[aria-label="Download ${name}"]`)!
        .click(),
    );
    expect(downloadFile).toHaveBeenCalledWith(name, { url: "blob:preview" });
    expect(fetchInboxMedia).toHaveBeenCalledTimes(1);
  },
);

it("keeps unsupported inbox content as a link without a download control", async () => {
  vi.mocked(fetchInboxMedia).mockResolvedValue(
    new TextEncoder().encode("<svg></svg>"),
  );
  await act(async () =>
    root.render(
      createElement(InboxMedia, {
        src: "https://github.com/user-attachments/example",
        alt: "Diagram",
      }),
    ),
  );
  expect(container.querySelector("a")?.textContent).toBe("Diagram");
  expect(container.querySelector("button")).toBeNull();
});

it.each(["/repo/image.png", "remote://machine/repo/image.png"])(
  "downloads local media from its original path %s",
  async (path) => {
    const onOpen = vi.fn();
    await act(async () =>
      root.render(
        createElement(LocalMedia, {
          path,
          label: "Image",
          onOpen,
          fallback: "Missing",
        }),
      ),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Download image.png"]')!
        .click(),
    );
    expect(downloadFile).toHaveBeenCalledWith("image.png", { path });
    expect(onOpen).not.toHaveBeenCalled();
  },
);

it("downloads local video from disk while streaming its preview", async () => {
  await act(async () =>
    root.render(
      createElement(LocalMedia, {
        path: "/video.mov",
        label: "Video",
        fallback: "Missing",
      }),
    ),
  );
  expect(container.querySelector("video")?.getAttribute("src")).toBe(
    "asset:/video.mov",
  );
  expect(readBinaryFile).not.toHaveBeenCalled();
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Download video.mov"]')!
      .click(),
  );
  expect(downloadFile).toHaveBeenCalledWith("video.mov", {
    path: "/video.mov",
  });
});

it("falls back when local media cannot be read", async () => {
  vi.mocked(readBinaryFile).mockRejectedValue(new Error("Missing"));
  await act(async () =>
    root.render(
      createElement(LocalMedia, {
        path: "/missing.png",
        label: "Image",
        fallback: "Missing file",
      }),
    ),
  );
  expect(container.textContent).toBe("Missing file");
  expect(container.querySelector("button")).toBeNull();
});

it.each(["/video.mov", "remote://machine/video.mp4", "/image.png"])(
  "keeps the path-backed download after a playback error for %s",
  async (path) => {
    await act(async () =>
      root.render(
        createElement(LocalMedia, {
          path,
          label: "Media",
          fallback: "Preview unavailable",
        }),
      ),
    );
    const preview = container.querySelector("video, img")!;
    expect(preview).not.toBeNull();
    act(() => preview.dispatchEvent(new Event("error")));
    expect(container.querySelector("video, img")).toBeNull();
    expect(container.textContent).toContain("Preview unavailable");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label^="Download "]')!
        .click(),
    );
    expect(downloadFile).toHaveBeenCalledWith(path.split("/").pop(), { path });
  },
);

it("omits the download when video stream authorization fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("Missing"));
  await act(async () =>
    root.render(
      createElement(LocalMedia, {
        path: "/missing.mov",
        label: "Video",
        fallback: "Missing file",
      }),
    ),
  );
  expect(container.textContent).toBe("Missing file");
  expect(container.querySelector("button")).toBeNull();
});
