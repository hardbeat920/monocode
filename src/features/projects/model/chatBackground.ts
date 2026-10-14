import { t } from "../../../shared/i18n/locale";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export async function pickAndSaveChatBackground(): Promise<string | null> {
  const sourcePath = await open({
    multiple: false,
    directory: false,
    title: t("projects.chatBackground.pickTitle"),
    filters: [
      {
        name: t("projects.pick.images"),
        extensions: ["png", "jpg", "jpeg", "gif", "webp"],
      },
    ],
  });
  if (typeof sourcePath !== "string" || !sourcePath) return null;
  return invoke<string>("save_chat_background", { sourcePath });
}

export function removeChatBackground(): Promise<void> {
  return invoke<void>("remove_chat_background");
}

export async function pickAndSaveProjectChatBackground(
  project: string,
): Promise<string | null> {
  const sourcePath = await open({
    multiple: false,
    directory: false,
    title: t("projects.chatBackground.pickProjectTitle"),
    filters: [
      {
        name: t("projects.pick.images"),
        extensions: ["png", "jpg", "jpeg", "gif", "webp"],
      },
    ],
  });
  if (typeof sourcePath !== "string" || !sourcePath) return null;
  return invoke<string>("save_project_chat_background", {
    project,
    sourcePath,
  });
}

export function clearProjectChatBackground(project: string): Promise<void> {
  return invoke<void>("remove_project_chat_background", { project });
}

export function projectChatBackgroundSrc(
  path: string,
  revision: number,
): string {
  return `${convertFileSrc(path)}?v=${revision}`;
}
