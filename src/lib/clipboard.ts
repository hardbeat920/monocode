import { invoke } from "@tauri-apps/api/core";
import type { Attachment } from "./session";

type CopiedFile = { name: string; mimeType: string; data: string };

/** HTML keeps arbitrary files together with text across MonoCode windows. */
export async function copyMessage(
  text: string,
  attachments: Attachment[] = [],
): Promise<void> {
  const files: CopiedFile[] = [];
  for (const attachment of attachments) {
    if (
      attachment.kind === "image" &&
      !attachment.data &&
      !attachment.previewUrl &&
      !attachment.copyFromPath
    )
      continue;
    try {
      let data = attachment.data;
      if (data === undefined && attachment.path) {
        data = await invoke<string>("read_file_base64", {
          path: attachment.path,
        });
      }
      if (data === undefined)
        throw new Error("File content is no longer available.");
      files.push({
        name: attachment.name,
        mimeType: attachment.mimeType,
        data,
      });
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      throw new Error(`Could not copy ${attachment.name}: ${reason}`);
    }
  }
  if (!files.length) return copyText(text);
  const escape = (value: string) =>
    value.replace(
      /[&<>"']/g,
      (char) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[char]!,
    );
  const html = `<div data-monocode-files="${encodeURIComponent(JSON.stringify(files))}"><pre>${escape(text)}</pre>${files
    .map((file) => {
      const src = `data:${escape(file.mimeType)};base64,${escape(file.data)}`;
      return file.mimeType.startsWith("image/")
        ? `<img src="${src}" alt="${escape(file.name)}">`
        : `<a href="${src}" download="${escape(file.name)}">${escape(file.name)}</a>`;
    })
    .join("")}</div>`;
  const formats: Record<string, Blob> = {
    "text/plain": new Blob([text], { type: "text/plain" }),
    "text/html": new Blob([html], { type: "text/html" }),
  };
  const png = files.find((file) => file.mimeType === "image/png");
  if (png)
    formats["image/png"] = new Blob(
      [Uint8Array.from(atob(png.data), (char) => char.charCodeAt(0))],
      { type: "image/png" },
    );
  await navigator.clipboard.write([new ClipboardItem(formats)]);
}

/** Only embedded bytes are accepted; clipboard HTML cannot request local paths. */
export function messageFilesFromClipboard(
  clipboard: Pick<DataTransfer, "getData">,
): File[] | null {
  try {
    const html = clipboard.getData("text/html");
    const doc = new DOMParser().parseFromString(html, "text/html");
    const payload = doc
      .querySelector("[data-monocode-files]")
      ?.getAttribute("data-monocode-files");
    if (!payload) return null;
    const files: unknown = JSON.parse(decodeURIComponent(payload));
    if (!Array.isArray(files) || files.length > 20) return null;
    return files.map((file) => {
      if (
        typeof file?.name !== "string" ||
        typeof file?.mimeType !== "string" ||
        typeof file?.data !== "string"
      )
        throw new Error("Invalid attachment");
      const bytes = Uint8Array.from(atob(file.data), (char) =>
        char.charCodeAt(0),
      );
      return new File([bytes], file.name, { type: file.mimeType });
    });
  } catch {
    return null;
  }
}

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const el = document.createElement("textarea");
    el.value = text;
    el.style.position = "fixed";
    el.style.left = "-9999px";
    document.body.appendChild(el);
    el.select();
    const ok = document.execCommand("copy");
    el.remove();
    if (!ok) throw new Error("copy failed");
  }
}
