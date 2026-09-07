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
 * The file a project is about to stop showing, or `null` when it must stay.
 *
 * Logos are filed on disk under a stem derived from the project key, so a logo
 * saved before the keys became paths sits under a stem nothing derives anymore
 * and the stored path is the only handle left to it. Migrated projects that
 * shared a folder name also share that one file, so it only goes when the last
 * project pointing at it lets go.
 */
export function droppableLogoFile(
  logos: Record<string, string>,
  project: string,
  keep?: string,
): string | null {
  const previous = logos[project];
  if (!previous || previous === keep) return null;
  const shared = Object.entries(logos).some(
    ([key, path]) => key !== project && path === previous,
  );
  return shared ? null : previous;
}

async function forgetLogoFile(path: string | null): Promise<void> {
  if (!path) return;
  await invoke("forget_logo_file", { path }).catch(() => undefined);
}

export async function pickAndSetProjectLogo(project: string): Promise<string | null> {
  const sourcePath = await pickImageFile();
  if (!sourcePath) return null;
  const logos = loadTabGroupLogos();
  const path = await invoke<string>("save_project_logo", {
    project,
    sourcePath,
  });
  await forgetLogoFile(droppableLogoFile(logos, project, path));
  saveTabGroupLogo(project, path);
  notifyTabGroupLogosChanged();
  return path;
}

export async function clearProjectLogo(project: string): Promise<void> {
  const stale = droppableLogoFile(loadTabGroupLogos(), project);
  await invoke("remove_project_logo", { project });
  await forgetLogoFile(stale);
  saveTabGroupLogo(project, null);
  notifyTabGroupLogosChanged();
}

export function projectLogoSrc(path: string | null | undefined): string | null {
  if (!path) return null;
  return `${convertFileSrc(path)}?v=${tabGroupLogoDisplayRevision()}`;
}
