import { ArrowDownCircle, Loader, RefreshCw, Settings } from "./icons";
import { useCallback, useEffect, useState } from "react";
import {
  installPendingUpdate,
  probeForUpdate,
  readAppVersion,
  runUpdateFlow,
  type UpdaterSnapshot,
} from "../lib/updater";
import type { InstalledUpdate } from "../lib/updateNotice";
import { MOD } from "../lib/platform";
import { RailAction } from "./RailAction";
import { UpdateRailCard } from "./UpdateRailCard";

export function SidebarUpdateFooter({
  update,
  onOpenWhatsNew,
  onDismissUpdate,
  onOpenSettings,
}: {
  update?: InstalledUpdate | null;
  onOpenWhatsNew?: (version: string) => void;
  onDismissUpdate?: () => void;
  onOpenSettings?: () => void;
}) {
  return (
    <div className="flex flex-col gap-1.5 p-2 pb-1">
      {update && onOpenWhatsNew && onDismissUpdate ? (
        <UpdateRailCard
          update={update}
          onOpen={onOpenWhatsNew}
          onDismiss={onDismissUpdate}
        />
      ) : null}
      <div className="flex items-center gap-1">
        <RailAction
          label="Settings"
          icon={Settings}
          onClick={onOpenSettings}
          shortcut={`${MOD},`}
          ariaLabel={`Settings (${MOD},)`}
        />
        <SidebarUpdate />
      </div>
    </div>
  );
}

export function SidebarUpdate() {
  const [snapshot, setSnapshot] = useState<UpdaterSnapshot>({
    phase: "idle",
    currentVersion: "…",
  });

  useEffect(() => {
    let cancelled = false;

    (async () => {
      const currentVersion = await readAppVersion();
      if (cancelled) return;
      setSnapshot({ phase: "checking", currentVersion });

      try {
        const update = await probeForUpdate();
        if (cancelled) return;
        if (update) {
          setSnapshot({
            phase: "available",
            currentVersion,
            availableVersion: update.version,
          });
          return;
        }
        setSnapshot({ phase: "current", currentVersion });
      } catch {
        if (cancelled) return;
        setSnapshot({ phase: "idle", currentVersion });
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  const onClick = useCallback(async () => {
    if (snapshot.phase === "downloading" || snapshot.phase === "checking") {
      return;
    }

    if (snapshot.phase === "available") {
      await installPendingUpdate(setSnapshot);
      return;
    }

    await runUpdateFlow(true, setSnapshot);
  }, [snapshot.phase]);

  const busy =
    snapshot.phase === "checking" || snapshot.phase === "downloading";
  const hasUpdate = snapshot.phase === "available";
  const label = hasUpdate
    ? `Update to ${snapshot.availableVersion}`
    : busy
      ? snapshot.phase === "downloading"
        ? `Downloading${snapshot.progress != null ? ` ${snapshot.progress}%` : "…"}`
        : "Checking…"
      : "Check for updates";
  const title = `${label} · v${snapshot.currentVersion}`;

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      title={title}
      aria-label={title}
      className={`grid size-8 shrink-0 place-items-center rounded-md transition-colors ${
        hasUpdate
          ? "bg-accent/15 text-content hover:bg-accent/20"
          : "text-content/50 hover:bg-content/10 hover:text-content"
      } disabled:cursor-default disabled:opacity-70`}
    >
      {busy ? (
        <Loader className="size-4 animate-spin opacity-70" aria-hidden />
      ) : hasUpdate ? (
        <ArrowDownCircle className="size-4 text-accent" aria-hidden />
      ) : (
        <RefreshCw
          className="size-4 opacity-70"
          strokeWidth={1.75}
          aria-hidden
        />
      )}
    </button>
  );
}
