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
  SaveFormat: { JPEG: "jpeg", PNG: "png" },
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

/** A clipboard PNG of about this many bytes, as the data URI expo-clipboard returns. */
const png = (bytes: number) => `data:image/png;base64,${"A".repeat(Math.ceil((bytes * 4) / 3))}`;
const MB = 1024 * 1024;

it.each([
  ["iPhone 15", 1179, 2556, 1.2 * MB],
  ["iPhone 15 Pro Max", 1290, 2796, 3 * MB],
  ["iPhone 16 Pro Max", 1320, 2868, 4 * MB],
  ["iPad Pro 13-inch", 2064, 2752, 4.5 * MB],
])("holds a real %s screenshot as the lossless PNG it is", async (_, width, height, bytes) => {
  runtime.image = { data: png(bytes), size: { width, height } };
  runtime.fileSize = bytes;
  const [attachment] = await pasteImage(10);
  expect(runtime.manipulated).toEqual([]);
  expect(attachment).toMatchObject({
    name: "Pasted image.png",
    mimeType: "image/png",
    data: runtime.image.data.slice("data:image/png;base64,".length),
  });
  expect(Math.abs(attachment.size - bytes)).toBeLessThan(4);
  // Its own data URI is the thumbnail: no cache file to write, or to leave behind.
  expect(attachment.uri).toBe(runtime.image.data);
  expect(runtime.written).toEqual([]);
});

it("scales and re-encodes a large pasted image as picked photos are", async () => {
  runtime.image = { data: png(5 * MB), size: { width: 3000, height: 4000 } };
  runtime.fileSize = 5 * MB;
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
  runtime.image = { data: png(5 * MB), size: { width: 3000, height: 4000 } };
  runtime.fileSize = 5 * MB;
  runtime.failRender = true;
  await expect(pasteImage(10)).rejects.toThrow("Could not decode");
  expect(runtime.deleted).toEqual([runtime.written[0].uri]);
});

it("re-encodes a screenshot-sized PNG too heavy to send as it is", async () => {
  runtime.image = { data: png(7 * MB), size: { width: 1290, height: 2796 } };
  runtime.fileSize = 7 * MB;
  const [attachment] = await pasteImage(10);
  expect(runtime.manipulated).toHaveLength(1);
  expect(attachment.mimeType).toBe("image/jpeg");
});

it("refuses an image too large to paste before writing or decoding it", async () => {
  // A 48 MP photo copied from Photos.
  runtime.image = { data: png(2 * MB), size: { width: 8064, height: 6048 } };
  await expect(pasteImage(10)).rejects.toThrow(/too large to paste.*Photo Library/);
  runtime.image = { data: png(40 * MB), size: { width: 3000, height: 4000 } };
  await expect(pasteImage(10)).rejects.toThrow(/too large to paste/);
  expect(runtime.written).toEqual([]);
  expect(runtime.manipulated).toEqual([]);
});

it("says so when there is no image to paste", async () => {
  await expect(pasteImage(10)).rejects.toThrow("no image on the clipboard");
  expect(runtime.written).toEqual([]);
});

it("takes a refused paste (iOS's Don't Allow) as a cancel, not an error", async () => {
  // The clipboard still has an image, but iOS gave none.
  runtime.hasImage = true;
  expect(await pasteImage(10)).toEqual([]);
});

it("explains an image the copying app no longer lets be read", async () => {
  const { getImageAsync } = await import("expo-clipboard");
  vi.mocked(getImageAsync).mockRejectedValueOnce(
    Object.assign(new Error("App has no permission to read this clipboard item"), { code: "ERR_NO_PERMISSION" }),
  );
  await expect(pasteImage(10)).rejects.toThrow("Copy it again");
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

it("drops a clipboard check that a newer change has overtaken", async () => {
  const { hasImageAsync } = await import("expo-clipboard");
  const held: ((has: boolean) => void)[] = [];
  vi.mocked(hasImageAsync).mockImplementation(() => new Promise((resolve) => held.push(resolve)));
  const seen: boolean[] = [];
  const stop = watchClipboardImage((has) => seen.push(has));
  // Text is copied before the mount's check answers; its late "image" must not win.
  runtime.clipboardListener!({ contentTypes: ["plain-text"] });
  held[0](true);
  await Promise.resolve();
  expect(seen.at(-1)).toBe(false);
  // Two foreground checks answering out of order: the later one's answer stands.
  runtime.appListener!("active");
  runtime.appListener!("active");
  held[2](true);
  held[1](false);
  await vi.waitFor(() => expect(seen.at(-1)).toBe(true));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(seen.at(-1)).toBe(true);
  stop();
});
