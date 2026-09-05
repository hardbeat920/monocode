import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { t } from "./i18n";
import {
  notifyTabGroupLogosChanged,
  saveTabGroupLogo,
  tabGroupLogoDisplayRevision,
} from "./tabGroups";

export async function pickImageFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    title: t("Choose project logo"),
    filters: [
      {
        name: t("Images"),
        extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg"],
      },
    ],
  });
  if (typeof selected === "string" && selected) return selected;
  return null;
}

export async function pickAndSetProjectLogo(project: string): Promise<string | null> {
  const sourcePath = await pickImageFile();
  if (!sourcePath) return null;
  const path = await invoke<string>("save_project_logo", {
    project,
    sourcePath,
  });
  saveTabGroupLogo(project, path);
  notifyTabGroupLogosChanged();
  return path;
}

export async function clearProjectLogo(project: string): Promise<void> {
  await invoke("remove_project_logo", { project });
  saveTabGroupLogo(project, null);
  notifyTabGroupLogosChanged();
}

export function projectLogoSrc(path: string | null | undefined): string | null {
  if (!path) return null;
  return `${convertFileSrc(path)}?v=${tabGroupLogoDisplayRevision()}`;
}
