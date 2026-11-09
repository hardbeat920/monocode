import {
  addClipboardListener,
  ContentType,
  getImageAsync,
  hasImageAsync,
} from "expo-clipboard";
import { File, Paths } from "expo-file-system";
import { ImageManipulator, SaveFormat } from "expo-image-manipulator";
import { launchImageLibraryAsync, type ImagePickerAsset } from "expo-image-picker";
import { AppState } from "react-native";

/** A file or photo held in the composer until it's sent. */
export type Attachment = {
  id: string;
  name: string;
  mimeType: string;
  size: number;
  /** On this phone, for the composer's thumbnail. */
  uri: string;
  /** The file's bytes, as the desktop takes them. */
  data: string;
};

/** What the desktop is sent for each: it stages them as its own composer does. */
export type WireAttachment = Pick<Attachment, "name" | "mimeType" | "data">;

export const MAX_ATTACHMENTS = 10;
/** Per file, before it travels as base64. */
export const MAX_FILE_BYTES = 10 * 1024 * 1024;
/** The whole message's, under the link's request limit with room for the text. */
const MAX_TOTAL_BASE64 = 24 * 1024 * 1024;
/** A photo's longer side; larger ones are scaled down, as vision models do anyway. */
const MAX_IMAGE_SIDE = 2048;
/** Sent as picked when already small and in a format every provider reads. */
const SENT_AS_IS = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);
const MAX_AS_IS_BYTES = 1.5 * 1024 * 1024;
/**
 * A PNG is usually a screenshot: text that JPEG smears. One up to a tall
 * iPhone's or an iPad's (2064 x 2752) is sent lossless, within a byte budget.
 */
const MAX_AS_IS_PNG_PIXELS = 6 * 1024 * 1024;
const MAX_AS_IS_PNG_SIDE = 4096;
const MAX_AS_IS_PNG_BYTES = 5 * 1024 * 1024;
/**
 * A pasted image arrives whole, as base64 on the JS thread; past these it is
 * refused before it's decoded or written. A 48 MP photo goes through Photo
 * Library instead, which never brings its bytes into JS.
 */
const MAX_PASTE_PIXELS = 25_000_000;
const MAX_PASTE_BYTES = 20 * 1024 * 1024;

const MIME_BY_EXT: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  heic: "image/heic",
  pdf: "application/pdf",
  txt: "text/plain",
  md: "text/markdown",
  csv: "text/csv",
  json: "application/json",
  html: "text/html",
  zip: "application/zip",
};

export const isImage = (attachment: Pick<Attachment, "mimeType">) =>
  attachment.mimeType.startsWith("image/");

export const toWire = (attachments: Attachment[]): WireAttachment[] =>
  attachments.map(({ name, mimeType, data }) => ({ name, mimeType, data }));

/** Photos from the library, scaled down and re-encoded where they need it. */
export async function pickPhotos(room: number): Promise<Attachment[]> {
  const result = await launchImageLibraryAsync({
    mediaTypes: ["images"],
    allowsMultipleSelection: true,
    selectionLimit: room,
    quality: 1,
  });
  if (result.canceled) return [];
  return Promise.all(result.assets.slice(0, room).map(fromPhoto));
}

/** Any files, from Files and its providers. */
export async function pickFiles(room: number): Promise<Attachment[]> {
  const picked = await File.pickFileAsync({ multipleFiles: true });
  if (picked.canceled) return [];
  const files = picked.result.slice(0, room);
  const tooLarge = files.find((file) => file.size > MAX_FILE_BYTES);
  if (tooLarge) throw new Error(`${tooLarge.name} is over the 10 MB limit.`);
  return Promise.all(
    files.map(async (file) => ({
      id: makeId(),
      name: file.name,
      mimeType: mimeFor(file.name, file.type),
      size: file.size,
      uri: file.uri,
      data: await file.base64(),
    })),
  );
}

/**
 * The clipboard's image (a screenshot, a photo copied from Photos), as if it
 * had been picked: the same scaling, re-encoding and limits. iOS asks the user
 * to allow each paste unless they've allowed it in Settings; a refusal is a
 * cancel, not an error.
 */
export async function pasteImage(room: number): Promise<Attachment[]> {
  if (room <= 0) return [];
  // Encoded and sent over on a native background thread, not the UI's.
  const image = await getImageAsync({ format: "png" }).catch((reason: unknown) => {
    throw pasteError(reason);
  });
  if (!image) {
    // Still an image there, but none given: the user chose Don't Allow.
    if (await hasImageAsync().catch(() => false)) return [];
    throw new Error("There's no image on the clipboard to paste.");
  }
  const { width, height } = image.size;
  // Arrives as a data URI; the bytes are what follows its prefix.
  const start = image.data.indexOf(",") + 1;
  const bytes = Math.floor(((image.data.length - start) * 3) / 4);
  if (width * height > MAX_PASTE_PIXELS || bytes > MAX_PASTE_BYTES)
    throw new Error(
      `This image is too large to paste (${Math.round(width)} x ${Math.round(height)}). ` +
        "Save it to Photos and attach it from Photo Library instead.",
    );
  const name = "Pasted image.png";
  if (heldAsIs("image/png", bytes, width, height))
    // Its data URI is the thumbnail: no cache file to write, or to clean up.
    return [{ id: makeId(), name, mimeType: "image/png", size: bytes, uri: image.data, data: image.data.slice(start) }];
  const file = new File(Paths.cache, `pasted-${makeId()}.png`);
  try {
    file.write(image.data.slice(start), { encoding: "base64" });
    return [await reencode(file.uri, width, height, name)];
  } finally {
    // The re-encoded JPEG is the attachment now, or nothing is.
    try {
      if (file.exists) file.delete();
    } catch {
      // Only a cache file; the OS clears the cache in time anyway.
    }
  }
}

/** A native paste error, in words for the user. */
function pasteError(reason: unknown): Error {
  const code = (reason as { code?: unknown } | null)?.code;
  if (code === "ERR_NO_PERMISSION")
    return new Error("The app this image was copied from no longer shares it. Copy it again and paste.");
  if (code === "ERR_PASTE_FAILURE")
    return new Error("The image on the clipboard couldn't be read. Copy it again and paste.");
  return reason instanceof Error ? reason : new Error(String(reason));
}

/**
 * Tells whether the clipboard holds an image: now, whenever it changes, and
 * on coming back to the app (copied elsewhere). Only asks, never reads, so iOS
 * doesn't prompt to allow a paste until one is chosen. An answer overtaken by
 * a newer check or change is dropped, so a slow one can't undo a later copy.
 */
export function watchClipboardImage(onChange: (hasImage: boolean) => void): () => void {
  let live = true;
  let latest = 0;
  const report = (turn: number, has: boolean) => {
    if (live && turn === latest) onChange(has);
  };
  const check = () => {
    const turn = ++latest;
    void hasImageAsync().then(
      (has) => report(turn, has),
      () => report(turn, false),
    );
  };
  check();
  const clipboard = addClipboardListener(({ contentTypes }) => {
    report(++latest, contentTypes.includes(ContentType.IMAGE));
  });
  const app = AppState.addEventListener("change", (state) => {
    if (state === "active") check();
  });
  return () => {
    live = false;
    clipboard.remove();
    app.remove();
  };
}

/** Adds picks to what's held, refusing any that would take the message over the link's limit. */
export function merge(existing: Attachment[], added: Attachment[]): Attachment[] {
  const next = [...existing, ...added].slice(0, MAX_ATTACHMENTS);
  const total = next.reduce((sum, attachment) => sum + attachment.data.length, 0);
  if (total > MAX_TOTAL_BASE64)
    throw new Error("These attachments are too large to send together. Remove some and try again.");
  return next;
}

type PhotoSource = Pick<ImagePickerAsset, "uri" | "width" | "height"> &
  Partial<Pick<ImagePickerAsset, "mimeType" | "fileName" | "fileSize">>;

async function fromPhoto(asset: PhotoSource): Promise<Attachment> {
  const mimeType = asset.mimeType ?? mimeFor(asset.fileName ?? "", null);
  if (heldAsIs(mimeType, asset.fileSize ?? Infinity, asset.width, asset.height)) {
    const file = new File(asset.uri);
    return {
      id: makeId(),
      name: asset.fileName ?? `photo.${mimeType.split("/")[1]}`,
      mimeType,
      size: file.size,
      uri: asset.uri,
      data: await file.base64(),
    };
  }
  return reencode(asset.uri, asset.width, asset.height, asset.fileName ?? "photo");
}

/** Whether an image goes as it is: small, or a screenshot-sized PNG, in a format every provider reads. */
function heldAsIs(mimeType: string, bytes: number, width: number, height: number): boolean {
  if (!SENT_AS_IS.has(mimeType)) return false;
  const side = Math.max(width, height);
  if (mimeType === "image/png" && bytes <= MAX_AS_IS_PNG_BYTES)
    return width * height <= MAX_AS_IS_PNG_PIXELS && side <= MAX_AS_IS_PNG_SIDE;
  return bytes <= MAX_AS_IS_BYTES && side <= MAX_IMAGE_SIDE;
}

/** Scaled down to the longer side's limit and re-encoded as JPEG, natively. */
async function reencode(uri: string, width: number, height: number, fileName: string): Promise<Attachment> {
  const context = ImageManipulator.manipulate(uri);
  if (Math.max(width, height) > MAX_IMAGE_SIDE)
    context.resize(width >= height ? { width: MAX_IMAGE_SIDE } : { height: MAX_IMAGE_SIDE });
  const saved = await (await context.renderAsync()).saveAsync({
    format: SaveFormat.JPEG,
    compress: 0.85,
    base64: true,
  });
  const base = fileName.replace(/\.[^.]+$/, "");
  return {
    id: makeId(),
    name: `${base}.jpg`,
    mimeType: "image/jpeg",
    size: Math.floor(((saved.base64 ?? "").length * 3) / 4),
    uri: saved.uri,
    data: saved.base64 ?? "",
  };
}

function mimeFor(name: string, type: string | null | undefined): string {
  if (type && type !== "application/octet-stream") return type;
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return MIME_BY_EXT[ext] ?? "application/octet-stream";
}

const makeId = () => `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
