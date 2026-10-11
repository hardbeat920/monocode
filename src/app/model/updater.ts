import { BundleType, getBundleType, getVersion } from "@tauri-apps/api/app";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { announceUpdateAvailable } from "../../features/settings/model/sounds";
import { quitWhileBusyMessage } from "../../features/sessions/model/inFlight";
import { liveInFlightCount } from "./appLifecycle";
import { rememberInstalledUpdate } from "./updateNotice";

export type UpdaterPhase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "error";

export type UpdaterSnapshot = {
  phase: UpdaterPhase;
  currentVersion: string;
  availableVersion?: string;
  progress?: number;
  error?: string;
  /** Set on Linux .deb/.rpm installs, which update through apt/dnf. */
  packageManaged?: PackageManagedInstall;
};

let pendingUpdate: Update | null = null;

const RELEASES_URL = "https://github.com/hardbeat920/monocode/releases/latest";

/**
 * Linux `.deb` and `.rpm` installs belong to apt/dnf. The release feed only
 * publishes an AppImage target, so the plugin reports no matching platform
 * for them; surface that as "update through your package manager" instead of
 * a raw error, and never self-install.
 */
export type PackageManagedInstall = "deb" | "rpm";

export async function packageManagedInstall(): Promise<PackageManagedInstall | null> {
  let type: string | null;
  try {
    type = await getBundleType();
  } catch {
    return null;
  }
  if (type === BundleType.Deb) return "deb";
  if (type === BundleType.Rpm) return "rpm";
  return null;
}

export function packageManagerHint(kind: PackageManagedInstall): string {
  return kind === "deb"
    ? `Download one .deb from ${RELEASES_URL} and run: sudo apt install ./MonoCode_X.Y.Z_amd64.deb\nReplace the file name with the one you downloaded.`
    : `Download one .rpm from ${RELEASES_URL} and run: sudo dnf install ./MonoCode-X.Y.Z-1.x86_64.rpm\nReplace the file name with the one you downloaded.`;
}

function isTargetMissingError(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  // tauri-plugin-updater Error::TargetsNotFound / Error::TargetNotFound.
  return /none of the fallback platforms|the platform `[^`]*` was not found/i.test(text);
}

function isUpdaterNotConfiguredError(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  return /updater does not have any endpoints set/i.test(text);
}

export async function readAppVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "0.0.0";
  }
}

export async function probeForUpdate(): Promise<Update | null> {
  if (await packageManagedInstall()) {
    pendingUpdate = null;
    return null;
  }
  const update = await check();
  pendingUpdate = update;
  if (update) announceUpdateAvailable(update.version);
  return update;
}

export async function runUpdateFlow(
  manual: boolean,
  onProgress?: (snapshot: UpdaterSnapshot) => void,
): Promise<UpdaterSnapshot> {
  const currentVersion = await readAppVersion();
  const base: UpdaterSnapshot = { phase: "checking", currentVersion };
  onProgress?.(base);

  const managed = await packageManagedInstall();
  if (managed) {
    pendingUpdate = null;
    const idle: UpdaterSnapshot = {
      phase: "idle",
      currentVersion,
      packageManaged: managed,
    };
    onProgress?.(idle);
    if (manual) {
      await message(packageManagerHint(managed), {
        title: "MonoCode",
      });
    }
    return idle;
  }

  try {
    const update = await check();
    if (!update) {
      pendingUpdate = null;
      const current: UpdaterSnapshot = { phase: "current", currentVersion };
      onProgress?.(current);
      if (manual) {
        await message("You're on the latest version.", { title: "MonoCode" });
      }
      return current;
    }

    pendingUpdate = update;
    announceUpdateAvailable(update.version);
    const available: UpdaterSnapshot = {
      phase: "available",
      currentVersion,
      availableVersion: update.version,
    };
    onProgress?.(available);

    if (!manual) return available;

    const notes = update.body?.trim();
    const detail = notes ? `\n\n${notes}` : "";
    const yes = await ask(
      `MonoCode ${update.version} is available (you have ${currentVersion}).${detail}\n\nInstall now?`,
      { title: "Update available", kind: "info" },
    );
    if (!yes) return available;

    return installPendingUpdate(onProgress);
  } catch (err) {
    if (isUpdaterNotConfiguredError(err)) {
      pendingUpdate = null;
      const idle: UpdaterSnapshot = { phase: "idle", currentVersion };
      onProgress?.(idle);
      if (manual) {
        await message(
          `Automatic updates aren't configured for this build.\n\nDownload releases at ${RELEASES_URL}`,
          { title: "MonoCode" },
        );
      }
      return idle;
    }

    if (isTargetMissingError(err)) {
      // The feed has no build for this platform/installer yet.
      pendingUpdate = null;
      const idle: UpdaterSnapshot = { phase: "idle", currentVersion };
      onProgress?.(idle);
      if (manual) {
        await message(
          `Automatic updates aren't available for this install yet.\n\nDownload releases at ${RELEASES_URL}`,
          { title: "MonoCode" },
        );
      }
      return idle;
    }

    const error = err instanceof Error ? err.message : String(err);
    const failed: UpdaterSnapshot = { phase: "error", currentVersion, error };
    onProgress?.(failed);
    if (manual) {
      await message(`Couldn't check for updates.\n\n${error}`, {
        title: "MonoCode",
      });
    }
    return failed;
  }
}

export async function installPendingUpdate(
  onProgress?: (snapshot: UpdaterSnapshot) => void,
): Promise<UpdaterSnapshot> {
  const currentVersion = await readAppVersion();
  const update = pendingUpdate;
  if (!update) {
    const idle: UpdaterSnapshot = { phase: "idle", currentVersion };
    onProgress?.(idle);
    return idle;
  }

  let downloaded = 0;
  let contentLength = 0;

  const downloading: UpdaterSnapshot = {
    phase: "downloading",
    currentVersion,
    availableVersion: update.version,
    progress: 0,
  };
  onProgress?.(downloading);

  try {
    await update.downloadAndInstall((event: DownloadEvent) => {
      if (event.event === "Started") {
        contentLength = event.data.contentLength ?? 0;
        downloaded = 0;
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
      }

      const progress =
        contentLength > 0
          ? Math.min(100, Math.round((downloaded / contentLength) * 100))
          : undefined;

      onProgress?.({
        phase: "downloading",
        currentVersion,
        availableVersion: update.version,
        progress,
      });
    });

    rememberInstalledUpdate(update.version);
    pendingUpdate = null;

    // The bits are already on disk; only the restart is optional. Ask before
    // taking it if chats are running, the same way a plain quit would — a
    // relaunch that starts unconditionally can't be walked back by a later
    // "No", since the new process is already on its way up by the time the
    // busy-quit dialog's answer comes back.
    const busy = liveInFlightCount();
    if (busy > 0) {
      const proceed = await ask(quitWhileBusyMessage(busy), {
        title: "MonoCode",
        kind: "warning",
        okLabel: "Quit",
      });
      if (!proceed) {
        return {
          phase: "current",
          currentVersion: update.version,
        };
      }
    }

    await relaunch();
    return {
      phase: "current",
      currentVersion: update.version,
    };
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    const failed: UpdaterSnapshot = {
      phase: "error",
      currentVersion,
      availableVersion: update.version,
      error,
    };
    onProgress?.(failed);
    await message(`Couldn't install the update.\n\n${error}`, { title: "MonoCode" });
    return failed;
  }
}
