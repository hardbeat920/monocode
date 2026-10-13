import { clearProjectChatBackground } from "../../projects/model/chatBackground";
import {
  clearProjectChatBackgroundSetting,
  loadProjectChatBackgroundSettings,
  type ProjectChatBackgroundSettings,
} from "../../projects/model/projectChatBackground";

/**
 * A Mono's background is stored like a project's, under a key no project
 * folder has, so the project background dialog can edit it as is.
 */
export function monoBackgroundKey(monoId: string): string {
  return `mono:${monoId}`;
}

/** A Mono uses its own background settings on its entire conversation. */
export function monoChatBackground(
  monoId: string,
): ProjectChatBackgroundSettings | null {
  const stored = loadProjectChatBackgroundSettings(monoBackgroundKey(monoId));
  if (!stored) return null;
  return {
    ...stored,
    scope: "all",
  };
}

/** Forgets the image along with its Mono. */
export function removeMonoBackground(monoId: string): void {
  const key = monoBackgroundKey(monoId);
  if (!loadProjectChatBackgroundSettings(key)) return;
  clearProjectChatBackgroundSetting(key);
  void clearProjectChatBackground(key).catch(() => {});
}
