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
  // The bytes follow the data URI's prefix, wrapped into lines on Android.
  const { data, wrapped } = clipboardBase64(image.data);
  // The PNG's own pixels: iOS reports its size in points, a third of them at 3x.
  const { width, height } = pngSize(data) ?? image.size;
  const bytes = base64Bytes(data);
  if (width * height > MAX_PASTE_PIXELS || bytes > MAX_PASTE_BYTES)
    throw new Error(
      `This image is too large to paste (${Math.round(width)} x ${Math.round(height)}). ` +
        "Save it to Photos and attach it from Photo Library instead.",
    );
  const name = "Pasted image.png";
  if (heldAsIs("image/png", bytes, width, height))
    // Its data URI is the thumbnail: no cache file to write, or to clean up.
    // (A cache file's URI would spare the Image a multi-MB string, at the cost
    // of deleting it on remove, send, failure and unmount; not done yet.)
    return [
      {
        id: makeId(),
        name,
        mimeType: "image/png",
        size: bytes,
        uri: wrapped ? `data:image/png;base64,${data}` : image.data,
        data,
      },
    ];
  const file = new File(Paths.cache, `pasted-${makeId()}.png`);
  try {
    file.write(data, { encoding: "base64" });
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

/** A data URI's base64, without the line breaks Android's clipboard puts in every 76 characters. */
function clipboardBase64(uri: string): { data: string; wrapped: boolean } {
  const body = uri.slice(uri.indexOf(",") + 1);
  // Unwrapped (iOS), the check scans the whole string once; wrapped, it stops at the first line.
  const wrapped = /\s/.test(body);
  return { data: wrapped ? body.replace(/\s+/g, "") : body, wrapped };
}

/** The bytes unbroken base64 decodes to: three for every four characters, less its padding. */
export function base64Bytes(data: string): number {
  const padding = data.endsWith("==") ? 2 : data.endsWith("=") ? 1 : 0;
  return Math.floor((data.length * 3) / 4) - padding;
}

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/**
 * A PNG's size in pixels, from its header: the signature, then the IHDR chunk
 * whose first eight bytes are the width and height. Only its first 24 bytes
 * (32 base64 characters) are decoded. Null when it isn't a PNG.
 */
export function pngSize(data: string): { width: number; height: number } | null {
  if (data.length < 32) return null;
  const bytes: number[] = [];
  for (let i = 0; i < 32; i += 4) {
    const [a, b, c, d] = [0, 1, 2, 3].map((j) => BASE64.indexOf(data[i + j]));
    if (a < 0 || b < 0 || c < 0 || d < 0) return null;
    const word = (a << 18) | (b << 12) | (c << 6) | d;
    bytes.push((word >> 16) & 0xff, (word >> 8) & 0xff, word & 0xff);
  }
  const ihdr = String.fromCharCode(...bytes.slice(12, 16));
  if (PNG_SIGNATURE.some((byte, i) => bytes[i] !== byte) || ihdr !== "IHDR") return null;
  const word = (at: number) =>
    ((bytes[at] << 24) | (bytes[at + 1] << 16) | (bytes[at + 2] << 8) | bytes[at + 3]) >>> 0;
  const width = word(16);
  const height = word(20);
  return width > 0 && height > 0 ? { width, height } : null;
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

/**
 * A failed send's attachments, back with any picked while it was sending: the
 * failed ones first, as the restored text refers to them, and each once. Those
 * picked since that no longer fit, by count or the link's limit, are dropped.
 */
export function restore(
  failed: Attachment[],
  since: Attachment[],
): { kept: Attachment[]; dropped: Attachment[] } {
  const seen = new Set(failed.map((attachment) => attachment.id));
  let kept = failed;
  const dropped: Attachment[] = [];
  for (const attachment of since) {
    if (seen.has(attachment.id)) continue;
    seen.add(attachment.id);
    try {
      const next = merge(kept, [attachment]);
      if (next.length > kept.length) {
        kept = next;
        continue;
      }
    } catch {
      // Over the link's limit with what's already kept.
    }
    dropped.push(attachment);
  }
  return { kept, dropped };
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
    size: base64Bytes(saved.base64 ?? ""),
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
