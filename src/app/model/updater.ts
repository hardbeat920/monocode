import { BundleType, getBundleType, getVersion } from "@tauri-apps/api/app";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  check,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";
import { announceUpdateAvailable } from "../../features/settings/model/sounds";
import { rememberInstalledUpdate } from "./updateNotice";

export type UpdaterPhase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "restart-required"
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
/** Version that finished installing but hasn't been restarted into yet. */
let pendingRestartVersion: string | null = null;

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
  return /none of the fallback platforms|the platform `[^`]*` was not found/i.test(
    text,
  );
}

function isUpdaterNotConfiguredError(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  return /updater does not have any endpoints set/i.test(text);
}

export function getPendingRestartVersion(): string | null {
  return pendingRestartVersion;
}

type PendingRestartListener = () => void;
const pendingRestartListeners = new Set<PendingRestartListener>();

/** Fires when the staged restart changes, so mounted rows can sync. */
export function subscribePendingRestart(
  listener: PendingRestartListener,
): () => void {
  pendingRestartListeners.add(listener);
  return () => {
    pendingRestartListeners.delete(listener);
  };
}

function setPendingRestartVersion(version: string | null): void {
  pendingRestartVersion = version;
  for (const listener of [...pendingRestartListeners]) {
    try {
      listener();
    } catch {
      // Listener failures stay silent; snapshot reads stay authoritative.
    }
  }
}

/**
 * UI callbacks must never break the non-throwing contract: every helper
 * below reports through snapshots/dialogs, so a throwing onProgress would
 * otherwise turn into an unhandled rejection in React onClick handlers.
 */
function emitProgress(
  onProgress: ((snapshot: UpdaterSnapshot) => void) | undefined,
  snapshot: UpdaterSnapshot,
): void {
  try {
    onProgress?.(snapshot);
  } catch {
    // Ignore UI callback failures; the returned snapshot stays authoritative.
  }
}

/** Relaunch into an already-installed update (user chose "Later" before).
 * Never rejects: a failed relaunch stays in `restart-required` and surfaces
 * a dialog, so React onClick handlers can't produce unhandled rejections. */
export async function restartToApplyUpdate(
  onProgress?: (snapshot: UpdaterSnapshot) => void,
): Promise<UpdaterSnapshot> {
  const currentVersion = await readAppVersion();
  const pending = pendingRestartVersion;
  if (!pending) {
    const idle: UpdaterSnapshot = { phase: "idle", currentVersion };
    emitProgress(onProgress, idle);
    return idle;
  }
  try {
    await relaunch();
    // relaunch should exit the process; if it returns, the restart is still
    // staged, so stay in restart-required and keep module state in sync.
    const staged: UpdaterSnapshot = {
      phase: "restart-required",
      currentVersion,
      availableVersion: pending,
    };
    emitProgress(onProgress, staged);
    return staged;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    const restartRequired: UpdaterSnapshot = {
      phase: "restart-required",
      currentVersion,
      availableVersion: pending,
      error,
    };
    emitProgress(onProgress, restartRequired);
    try {
      await message(`Couldn't restart to apply the update.\n\n${error}`, {
        title: "MonoCode",
      });
    } catch {
      // Dialog failures stay silent; the snapshot already carries the error.
    }
    return restartRequired;
  }
}

async function askToRestartNow(version: string): Promise<boolean> {
  try {
    return await ask(
      `MonoCode ${version} is installed and ready.\n\nRestart now to apply the update? You can also choose Later and restart whenever you're ready.`,
      {
        title: "Update ready",
        kind: "info",
        okLabel: "Restart now",
        cancelLabel: "Later",
      },
    );
  } catch {
    return false;
  }
}

export async function readAppVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "0.0.0";
  }
}

export async function probeForUpdate(): Promise<Update | null> {
  if (pendingRestartVersion) return null;
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
  emitProgress(onProgress, base);

  const managed = await packageManagedInstall();
  if (managed) {
    pendingUpdate = null;
    const idle: UpdaterSnapshot = {
      phase: "idle",
      currentVersion,
      packageManaged: managed,
    };
    emitProgress(onProgress, idle);
    if (manual) {
      await message(packageManagerHint(managed), {
        title: "MonoCode",
      });
    }
    return idle;
  }

  // An update already finished installing but the user chose Later. Don't
  // re-download: surface the pending restart instead, and let a manual check
  // re-offer the restart confirmation.
  if (pendingRestartVersion) {
    const restartRequired: UpdaterSnapshot = {
      phase: "restart-required",
      currentVersion,
      availableVersion: pendingRestartVersion,
    };
    emitProgress(onProgress, restartRequired);
    if (manual) {
      const restartNow = await askToRestartNow(pendingRestartVersion);
      if (restartNow) {
        return restartToApplyUpdate(onProgress);
      }
    }
    return restartRequired;
  }

  try {
    const update = await check();
    if (!update) {
      pendingUpdate = null;
      const current: UpdaterSnapshot = { phase: "current", currentVersion };
      emitProgress(onProgress, current);
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
    emitProgress(onProgress, available);

    if (!manual) return available;

    const notes = update.body?.trim();
    const detail = notes ? `\n\n${notes}` : "";
    const yes = await ask(
      `MonoCode ${update.version} is available (you have ${currentVersion}).${detail}\n\nInstall now?`,
      {
        title: "Update available",
        kind: "info",
        okLabel: "Install now",
        cancelLabel: "Later",
      },
    );
    if (!yes) return available;

    return installPendingUpdate(onProgress);
  } catch (err) {
    if (isUpdaterNotConfiguredError(err)) {
      pendingUpdate = null;
      const idle: UpdaterSnapshot = { phase: "idle", currentVersion };
      emitProgress(onProgress, idle);
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
      emitProgress(onProgress, idle);
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
    emitProgress(onProgress, failed);
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
    if (pendingRestartVersion) {
      const restartRequired: UpdaterSnapshot = {
        phase: "restart-required",
        currentVersion,
        availableVersion: pendingRestartVersion,
      };
      emitProgress(onProgress, restartRequired);
      // Stale "available" snapshot: re-offer so one click restarts.
      const restartNow = await askToRestartNow(pendingRestartVersion);
      if (restartNow) {
        return restartToApplyUpdate(onProgress);
      }
      return restartRequired;
    }
    const idle: UpdaterSnapshot = { phase: "idle", currentVersion };
    emitProgress(onProgress, idle);
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
  emitProgress(onProgress, downloading);

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

      emitProgress(onProgress, {
        phase: "downloading",
        currentVersion,
        availableVersion: update.version,
        progress,
      });
    });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    const failed: UpdaterSnapshot = {
      phase: "error",
      currentVersion,
      availableVersion: update.version,
      error,
    };
    emitProgress(onProgress, failed);
    try {
      await message(`Couldn't install the update.\n\n${error}`, {
        title: "MonoCode",
      });
    } catch {
      // The snapshot already carries the error.
    }
    return failed;
  }

  rememberInstalledUpdate(update.version);
  pendingUpdate = null;
  setPendingRestartVersion(update.version);

  const restartNow = await askToRestartNow(update.version);
  if (!restartNow) {
    const restartRequired: UpdaterSnapshot = {
      phase: "restart-required",
      currentVersion,
      availableVersion: update.version,
    };
    emitProgress(onProgress, restartRequired);
    return restartRequired;
  }

  return restartToApplyUpdate(onProgress);
}
