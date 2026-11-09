import { afterEach, beforeEach, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({
  image: null as null | { data: string; size: { width: number; height: number } },
  hasImage: false,
  written: [] as { uri: string; content: string; options: unknown }[],
  fileSize: 0,
  clipboardListener: null as null | ((event: { contentTypes: string[] }) => void),
  appListener: null as null | ((state: string) => void),
  removed: [] as string[],
  deleted: [] as string[],
  failRender: false,
  manipulated: [] as { uri: string; resize: unknown }[],
}));

vi.mock("expo-clipboard", () => ({
  ContentType: { IMAGE: "image", PLAIN_TEXT: "plain-text" },
  getImageAsync: vi.fn(async () => runtime.image),
  hasImageAsync: vi.fn(async () => runtime.hasImage),
  addClipboardListener: (listener: (event: { contentTypes: string[] }) => void) => {
    runtime.clipboardListener = listener;
    return { remove: () => runtime.removed.push("clipboard") };
  },
}));
vi.mock("react-native", () => ({
  AppState: {
    addEventListener: (_: string, listener: (state: string) => void) => {
      runtime.appListener = listener;
      return { remove: () => runtime.removed.push("app") };
    },
  },
}));
vi.mock("expo-file-system", () => {
  class File {
    uri: string;
    constructor(...parts: (string | { uri: string })[]) {
      this.uri = parts.map((part) => (typeof part === "string" ? part : part.uri)).join("/");
    }
    write(content: string, options: unknown) {
      runtime.written.push({ uri: this.uri, content, options });
    }
    get exists() {
      return runtime.written.some((item) => item.uri === this.uri);
    }
    delete() {
      runtime.deleted.push(this.uri);
    }
    get size() {
      return runtime.fileSize;
    }
    async base64() {
      return runtime.written.find((item) => item.uri === this.uri)?.content ?? "";
    }
  }
  return { File, Paths: { cache: { uri: "file:///cache" } } };
});
vi.mock("expo-image-manipulator", () => ({
  SaveFormat: { JPEG: "jpeg" },
  ImageManipulator: {
    manipulate: (uri: string) => {
      const entry = { uri, resize: null as unknown };
      runtime.manipulated.push(entry);
      const context = {
        resize: (size: unknown) => {
          entry.resize = size;
          return context;
        },
        renderAsync: async () => {
          if (runtime.failRender) throw new Error("Could not decode");
          return {
            saveAsync: async () => ({ uri: "file:///cache/out.jpg", base64: "SlBFRw==" }),
          };
        },
      };
      return context;
    },
  },
}));
vi.mock("expo-image-picker", () => ({ launchImageLibraryAsync: vi.fn() }));

import { pasteImage, watchClipboardImage } from "@/features/sessions/attachments";

beforeEach(() => {
  runtime.image = null;
  runtime.hasImage = false;
  runtime.written = [];
  runtime.fileSize = 0;
  runtime.removed = [];
  runtime.manipulated = [];
  runtime.deleted = [];
  runtime.failRender = false;
});
afterEach(() => vi.clearAllMocks());

it("holds a small pasted screenshot as the PNG it is", async () => {
  runtime.image = { data: "data:image/png;base64,UE5H", size: { width: 1179, height: 2000 } };
  runtime.fileSize = 3;
  const [attachment] = await pasteImage(10);
  expect(runtime.written[0]).toMatchObject({ content: "UE5H", options: { encoding: "base64" } });
  expect(runtime.written[0].uri).toMatch(/^file:\/\/\/cache\/pasted-.+\.png$/);
  expect(attachment).toMatchObject({
    name: "Pasted image.png",
    mimeType: "image/png",
    size: 3,
    uri: runtime.written[0].uri,
    data: "UE5H",
  });
  expect(runtime.manipulated).toEqual([]);
  // Kept: it is the attachment's thumbnail.
  expect(runtime.deleted).toEqual([]);
});

it("scales and re-encodes a large pasted image as picked photos are", async () => {
  runtime.image = { data: "data:image/png;base64,QklH", size: { width: 3000, height: 4000 } };
  runtime.fileSize = 5 * 1024 * 1024;
  const [attachment] = await pasteImage(10);
  expect(runtime.manipulated[0].resize).toEqual({ height: 2048 });
  expect(attachment).toMatchObject({
    name: "Pasted image.jpg",
    mimeType: "image/jpeg",
    uri: "file:///cache/out.jpg",
    data: "SlBFRw==",
  });
  // The pasted PNG gives way to the re-encoded JPEG rather than lingering in the cache.
  expect(runtime.deleted).toEqual([runtime.written[0].uri]);
});

it("leaves no pasted file behind when it can't be attached", async () => {
  runtime.image = { data: "data:image/png;base64,QklH", size: { width: 3000, height: 4000 } };
  runtime.fileSize = 5 * 1024 * 1024;
  runtime.failRender = true;
  await expect(pasteImage(10)).rejects.toThrow("Could not decode");
  expect(runtime.deleted).toEqual([runtime.written[0].uri]);
});

it("says so when there is no image to paste, or the paste was refused", async () => {
  await expect(pasteImage(10)).rejects.toThrow("no image on the clipboard");
  expect(runtime.written).toEqual([]);
});

it("pastes nothing without room", async () => {
  runtime.image = { data: "data:image/png;base64,UE5H", size: { width: 10, height: 10 } };
  expect(await pasteImage(0)).toEqual([]);
});

it("tracks whether the clipboard has an image without reading it", async () => {
  const { getImageAsync } = await import("expo-clipboard");
  const seen: boolean[] = [];
  runtime.hasImage = true;
  const stop = watchClipboardImage((has) => seen.push(has));
  await vi.waitFor(() => expect(seen).toEqual([true]));
  runtime.clipboardListener!({ contentTypes: ["plain-text"] });
  expect(seen).toEqual([true, false]);
  runtime.clipboardListener!({ contentTypes: ["image"] });
  expect(seen).toEqual([true, false, true]);
  runtime.hasImage = false;
  runtime.appListener!("active");
  await vi.waitFor(() => expect(seen).toEqual([true, false, true, false]));
  expect(getImageAsync).not.toHaveBeenCalled();
  stop();
  expect(runtime.removed).toEqual(["clipboard", "app"]);
});
