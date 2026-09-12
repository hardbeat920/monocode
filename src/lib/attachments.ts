import { invoke } from "@tauri-apps/api/core";
import { basename, pickFiles as pickFilePaths } from "./fs";
import type { Attachment, AttachmentKind } from "./session";

export const MAX_ATTACHMENTS = 20;
export const MAX_EMBED_BYTES = 20 * 1024 * 1024;

type PathInfo = {
  path: string;
  name: string;
  size: number;
  isDir: boolean;
};

type NativeFile = File & { path?: string };

export type PromptContentBlock =
  | { type: "text"; text: string }
  | { type: "image"; mimeType: string; data: string; uri?: string }
  | {
      type: "resource_link";
      uri: string;
      name: string;
      mimeType?: string;
      size?: number;
    };

const SKIP_NAMES = new Set([".ds_store", "thumbs.db", "desktop.ini"]);

/** MIME types providers typically send as vision input. */
const VISION_MIME = new Set([
  "image/png",
  "image/jpeg",
  "image/jpg",
  "image/gif",
  "image/webp",
]);

const MIME_BY_EXT: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  ico: "image/x-icon",
  svg: "image/svg+xml",
  tif: "image/tiff",
  tiff: "image/tiff",
  heic: "image/heic",
  heif: "image/heif",
  avif: "image/avif",
  mp3: "audio/mpeg",
  wav: "audio/wav",
  m4a: "audio/mp4",
  aac: "audio/aac",
  ogg: "audio/ogg",
  flac: "audio/flac",
  mp4: "video/mp4",
  mov: "video/quicktime",
  webm: "video/webm",
  mkv: "video/x-matroska",
  pdf: "application/pdf",
  txt: "text/plain",
  md: "text/markdown",
  markdown: "text/markdown",
  csv: "text/csv",
  tsv: "text/tab-separated-values",
  html: "text/html",
  htm: "text/html",
  xml: "application/xml",
  json: "application/json",
  yaml: "text/yaml",
  yml: "text/yaml",
  toml: "application/toml",
  rtf: "application/rtf",
  doc: "application/msword",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  xls: "application/vnd.ms-excel",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ppt: "application/vnd.ms-powerpoint",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  zip: "application/zip",
  gz: "application/gzip",
  tar: "application/x-tar",
  ts: "text/plain",
  tsx: "text/plain",
  js: "text/javascript",
  jsx: "text/plain",
  mjs: "text/javascript",
  cjs: "text/javascript",
  css: "text/css",
  rs: "text/plain",
  py: "text/x-python",
  go: "text/plain",
  java: "text/plain",
  kt: "text/plain",
  swift: "text/plain",
  c: "text/plain",
  h: "text/plain",
  cc: "text/plain",
  cpp: "text/plain",
  hpp: "text/plain",
  cs: "text/plain",
  rb: "text/plain",
  php: "text/plain",
  sh: "text/plain",
  zsh: "text/plain",
  bash: "text/plain",
  sql: "application/sql",
  graphql: "application/graphql",
};

export function persistableAttachment(file: Attachment): Attachment {
  return {
    id: file.id,
    name: file.name,
    mimeType: file.mimeType,
    kind: file.kind,
    size: file.size,
    ...(file.path ? { path: file.path } : {}),
  };
}

export function displayAttachments(files: Attachment[]): Attachment[] {
  return files.map((file) => ({
    ...persistableAttachment(file),
    ...(file.previewUrl ? { previewUrl: file.previewUrl } : {}),
    ...(file.data ? { data: file.data } : {}),
  }));
}

export function attachmentPreviewSrc(file: Attachment): string | undefined {
  if (file.previewUrl) return file.previewUrl;
  if (file.data && file.kind === "image") {
    return `data:${file.mimeType};base64,${file.data}`;
  }
  return undefined;
}

export function revokeAttachment(file: Attachment) {
  if (file.previewUrl) URL.revokeObjectURL(file.previewUrl);
}

export function mergeAttachments(
  existing: Attachment[],
  incoming: Attachment[],
): Attachment[] {
  const next = [...existing];
  for (const file of incoming) {
    const duplicate = next.some(
      (item) =>
        (item.path && file.path && item.path === file.path) ||
        item.id === file.id,
    );
    if (duplicate) continue;
    next.push(file);
    if (next.length >= MAX_ATTACHMENTS) break;
  }
  return next;
}

type ClipboardFileItem = {
  kind: string;
  type: string;
  getAsFile: () => File | null;
};

/** Clipboard files plus items. WebKit's FileList is often truncated to the first file. */
export function filesFromClipboard(
  data:
    | {
        files?: ArrayLike<File> | null;
        items?: ArrayLike<ClipboardFileItem> | null;
      }
    | null
    | undefined,
): File[] {
  const fromList = arrayLike(data?.files);
  const fromItems: File[] = [];
  for (const item of arrayLike(data?.items)) {
    if (item.kind !== "file") continue;
    const file = item.getAsFile();
    if (file) fromItems.push(file);
  }
  const files = fromItems.length > fromList.length ? fromItems : fromList;
  return dropMacScreenshotTwins(files);
}

function arrayLike<T>(list: ArrayLike<T> | null | undefined): T[] {
  return list ? Array.from(list) : [];
}

/** macOS often exposes the same screenshot as PNG and an unnamed TIFF. */
function dropMacScreenshotTwins(files: File[]): File[] {
  const unnamedTiff = (file: File) => {
    const type = file.type.toLowerCase();
    if (type !== "image/tiff" && type !== "image/tif") return false;
    const name = file.name.trim().toLowerCase();
    return (
      !name || name === "image.tiff" || name === "image.tif" || name === "image"
    );
  };
  const hasOtherImage = files.some(
    (file) => file.type.startsWith("image/") && !unnamedTiff(file),
  );
  if (!hasOtherImage) return files;
  return files.filter((file) => !unnamedTiff(file));
}

export async function pickAttachments(): Promise<Attachment[]> {
  const paths = await pickFilePaths();
  if (!paths?.length) return [];
  return attachmentsFromPaths(paths);
}

export async function attachmentsFromPaths(
  paths: string[],
): Promise<Attachment[]> {
  const unique = [...new Set(paths.filter((path) => path.trim()))];
  if (unique.length === 0) return [];
  const infos = await invoke<PathInfo[]>("inspect_paths", { paths: unique });
  const out: Attachment[] = [];
  for (const info of infos) {
    const file = await attachmentFromPath(info);
    if (file) out.push(file);
  }
  return out;
}

export async function attachmentsFromFiles(
  files: File[],
): Promise<Attachment[]> {
  const out: Attachment[] = [];
  const pathFiles: string[] = [];
  const blobs: File[] = [];
  for (const file of files) {
    const path = nativePath(file);
    if (path) pathFiles.push(path);
    else blobs.push(file);
  }
  if (pathFiles.length) {
    out.push(...(await attachmentsFromPaths(pathFiles)));
  }
  for (const file of blobs) {
    const item = await attachmentFromBlob(file);
    if (item) out.push(item);
  }
  return out;
}

export async function prepareAttachments(
  files: Attachment[],
): Promise<Attachment[]> {
  return Promise.all(
    files.map(async (file) => {
      if (file.data || !file.path) return file;
      if (!isVisionImage(file.mimeType) || file.size > MAX_EMBED_BYTES) {
        return file;
      }
      try {
        const data = await invoke<string>("read_file_base64", {
          path: file.path,
        });
        return { ...file, data };
      } catch {
        return file;
      }
    }),
  );
}

export function promptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  const blocks: PromptContentBlock[] = [];
  const trimmed = text.trim();
  if (trimmed) blocks.push({ type: "text", text: trimmed });
  for (const file of attachments) {
    blocks.push(contentBlockFor(file));
  }
  return blocks;
}

/** Require a deliverable source instead of silently dropping an attachment. */
export function attachmentPath(file: Attachment): string {
  if (!file.path?.trim()) {
    throw new Error(
      `Cannot attach ${JSON.stringify(file.name)}: no local file path is available. Attach the file again.`,
    );
  }
  return file.path;
}

/** Native harnesses without file blocks can ask their tools to read this path. */
export function attachmentPathText(file: Attachment): string {
  return `Attached file (read from disk): ${JSON.stringify(attachmentPath(file))}`;
}

function contentBlockFor(file: Attachment): PromptContentBlock {
  if (file.data && isVisionImage(file.mimeType)) {
    return {
      type: "image",
      mimeType: normalizeImageMime(file.mimeType),
      data: file.data,
      ...(file.path ? { uri: fileUri(file.path) } : {}),
    };
  }
  return {
    type: "resource_link",
    uri: fileUri(attachmentPath(file)),
    name: file.name,
    mimeType: file.mimeType,
    size: file.size,
  };
}

async function attachmentFromPath(info: PathInfo): Promise<Attachment | null> {
  if (info.isDir || skipName(info.name)) return null;
  const mimeType = mimeFromName(info.name);
  const kind = kindFromMime(mimeType);
  const file: Attachment = {
    id: crypto.randomUUID(),
    name: info.name,
    mimeType,
    kind,
    size: info.size,
    path: info.path,
  };
  if (
    isVisionImage(mimeType) &&
    info.size > 0 &&
    info.size <= MAX_EMBED_BYTES
  ) {
    try {
      file.data = await invoke<string>("read_file_base64", { path: info.path });
    } catch {
      // Fall back to a resource_link so the agent can still read the file.
    }
  }
  return file;
}

async function attachmentFromBlob(file: File): Promise<Attachment | null> {
  if (skipName(file.name) || file.size < 0) return null;
  const mimeType = mimeFromFile(file);
  const kind = kindFromMime(mimeType);
  const name = file.name.trim() || fallbackName(mimeType);
  const previewUrl = kind === "image" ? URL.createObjectURL(file) : undefined;
  const data = await readBlobBase64(file);
  if (kind === "image" && isVisionImage(mimeType) && data) {
    return {
      id: crypto.randomUUID(),
      name,
      mimeType: normalizeImageMime(mimeType),
      kind,
      size: file.size,
      data,
      previewUrl,
    };
  }
  if (!data) {
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    return null;
  }
  try {
    const path = await invoke<string>("write_attachment", { name, data });
    return {
      id: crypto.randomUUID(),
      name,
      mimeType,
      kind,
      size: file.size,
      path,
      previewUrl,
    };
  } catch {
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    return null;
  }
}

function mimeFromFile(file: File): string {
  const fromName = mimeFromName(file.name);
  if (file.type && file.type !== "application/octet-stream") {
    if (fromName !== "application/octet-stream") return fromName;
    return file.type;
  }
  return fromName;
}

function mimeFromName(name: string): string {
  const ext = extension(name);
  if (!ext) return "application/octet-stream";
  return (
    MIME_BY_EXT[ext] ??
    (isTextExt(ext) ? "text/plain" : "application/octet-stream")
  );
}

function isTextExt(ext: string): boolean {
  return [
    "txt",
    "md",
    "rst",
    "log",
    "cfg",
    "ini",
    "env",
    "lock",
    "gradle",
    "cmake",
    "mk",
    "vue",
    "svelte",
    "astro",
    "scss",
    "sass",
    "less",
    "lua",
    "r",
    "jl",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "clj",
    "scala",
    "groovy",
    "dart",
    "nim",
    "zig",
    "proto",
    "graphqls",
  ].includes(ext);
}

function kindFromMime(mimeType: string): AttachmentKind {
  if (mimeType.startsWith("image/")) return "image";
  if (mimeType.startsWith("audio/")) return "audio";
  return "file";
}

export function isVisionImage(mimeType: string): boolean {
  return VISION_MIME.has(mimeType.toLowerCase());
}

export function normalizeImageMime(mimeType: string): string {
  const mime = mimeType.toLowerCase();
  if (mime === "image/jpg") return "image/jpeg";
  return mime;
}

function extension(name: string): string {
  const base = basename(name).toLowerCase();
  const dot = base.lastIndexOf(".");
  if (dot <= 0 || dot === base.length - 1) return "";
  return base.slice(dot + 1);
}

function skipName(name: string): boolean {
  return SKIP_NAMES.has(basename(name).toLowerCase());
}

function nativePath(file: File): string | undefined {
  const path = (file as NativeFile).path;
  return path?.trim() ? path : undefined;
}

function fallbackName(mimeType: string): string {
  if (mimeType === "image/png") return "image.png";
  if (mimeType === "image/jpeg" || mimeType === "image/jpg") return "image.jpg";
  if (mimeType === "image/gif") return "image.gif";
  if (mimeType === "image/webp") return "image.webp";
  if (mimeType.startsWith("image/")) return "image";
  if (mimeType.startsWith("audio/")) return "audio";
  return "attachment";
}

function fileUri(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const abs = normalized.startsWith("/") ? normalized : `/${normalized}`;
  return `file://${abs.split("/").map(encodeURIComponent).join("/")}`;
}

function readBlobBase64(file: File): Promise<string | null> {
  if (file.size > MAX_EMBED_BYTES) return Promise.resolve(null);
  return new Promise((resolve) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        resolve(null);
        return;
      }
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => resolve(null);
    reader.readAsDataURL(file);
  });
}
