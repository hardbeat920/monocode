import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { invokeWorkspace, REMOTE_PATH_PREFIX } from "./fs";
import { readBlobBase64 } from "../../shared/lib/blobBase64";

export type DownloadSource =
  { path: string } | { url: string } | { text: string };

/** Save on this computer, even when the source belongs to a connected machine. */
export async function downloadFile(
  name: string,
  source: DownloadSource,
): Promise<void> {
  const destination = await save({
    title: "Download file",
    defaultPath: name.replace(/.*[/\\]/, "") || "download",
  });
  if (!destination) return;
  if ("path" in source && source.path.startsWith(REMOTE_PATH_PREFIX)) {
    await invokeWorkspace("download_file", { path: source.path, destination });
    return;
  }
  if (!("url" in source)) {
    await invoke("save_download", { destination, source });
    return;
  }
  const response = await fetch(source.url);
  if (!response.ok)
    throw new Error(`Could not read file (${response.status}).`);
  // Match write_attachment's transport and browser-native base64 encoding.
  const data = await readBlobBase64(await response.blob());
  if (data === null) throw new Error("Could not read file for download.");
  await invoke("save_download", { destination, source: { data } });
}
