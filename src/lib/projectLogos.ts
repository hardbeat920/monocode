import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  loadTabGroupLogos,
  notifyTabGroupLogosChanged,
  saveTabGroupLogo,
  tabGroupLogoDisplayRevision,
} from "./tabGroups";

export async function pickImageFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    title: "Choose project logo",
    filters: [
      {
        name: "Images",
        extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg"],
      },
    ],
  });
  if (typeof selected === "string" && selected) return selected;
  return null;
}

/**
 * Logos are filed on disk under a stem derived from the project key, so a logo
 * saved before the keys became paths sits under a stem nothing derives anymore.
 * The stored path is the only handle left to it.
 */
async function forgetLogoFile(
  previous: string | undefined,
  keep?: string,
): Promise<void> {
  if (!previous || previous === keep) return;
  await invoke("forget_logo_file", { path: previous }).catch(() => undefined);
}

export async function pickAndSetProjectLogo(project: string): Promise<string | null> {
  const sourcePath = await pickImageFile();
  if (!sourcePath) return null;
  const previous = loadTabGroupLogos()[project];
  const path = await invoke<string>("save_project_logo", {
    project,
    sourcePath,
  });
  await forgetLogoFile(previous, path);
  saveTabGroupLogo(project, path);
  notifyTabGroupLogosChanged();
  return path;
}

export async function clearProjectLogo(project: string): Promise<void> {
  const previous = loadTabGroupLogos()[project];
  await invoke("remove_project_logo", { project });
  await forgetLogoFile(previous);
  saveTabGroupLogo(project, null);
  notifyTabGroupLogosChanged();
}

export function projectLogoSrc(path: string | null | undefined): string | null {
  if (!path) return null;
  return `${convertFileSrc(path)}?v=${tabGroupLogoDisplayRevision()}`;
}
