import { File } from "expo-file-system";
import { ImageManipulator, SaveFormat } from "expo-image-manipulator";
import { launchImageLibraryAsync, type ImagePickerAsset } from "expo-image-picker";

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

/** Adds picks to what's held, refusing any that would take the message over the link's limit. */
export function merge(existing: Attachment[], added: Attachment[]): Attachment[] {
  const next = [...existing, ...added].slice(0, MAX_ATTACHMENTS);
  const total = next.reduce((sum, attachment) => sum + attachment.data.length, 0);
  if (total > MAX_TOTAL_BASE64)
    throw new Error("These attachments are too large to send together. Remove some and try again.");
  return next;
}

async function fromPhoto(asset: ImagePickerAsset): Promise<Attachment> {
  const mimeType = asset.mimeType ?? mimeFor(asset.fileName ?? "", null);
  const small =
    SENT_AS_IS.has(mimeType) &&
    (asset.fileSize ?? Infinity) <= MAX_AS_IS_BYTES &&
    Math.max(asset.width, asset.height) <= MAX_IMAGE_SIDE;
  if (small) {
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
  const context = ImageManipulator.manipulate(asset.uri);
  if (Math.max(asset.width, asset.height) > MAX_IMAGE_SIDE)
    context.resize(
      asset.width >= asset.height ? { width: MAX_IMAGE_SIDE } : { height: MAX_IMAGE_SIDE },
    );
  const saved = await (await context.renderAsync()).saveAsync({
    format: SaveFormat.JPEG,
    compress: 0.85,
    base64: true,
  });
  const base = (asset.fileName ?? "photo").replace(/\.[^.]+$/, "");
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
