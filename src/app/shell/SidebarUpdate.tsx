import { ArrowDownCircle, Loader, X } from "../../shared/ui/icons";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  getPendingRestartVersion,
  installPendingUpdate,
  probeForUpdate,
  readAppVersion,
  restartToApplyUpdate,
  subscribePendingRestart,
  type UpdaterSnapshot,
} from "../model/updater";
import type { InstalledUpdate } from "../model/updateNotice";
import { UpdateRailCard } from "./UpdateRailCard";

// The sidebar row only earns its space when there is something to act on: an
// update waiting to be installed, one already downloading, or one installed
// and waiting for a restart. Every other phase — including a probe that
// failed — stays silent, because manual "Check for updates" already lives in
// Settings and the app menu.
export function isSidebarUpdateActionable(snapshot: UpdaterSnapshot): boolean {
  return (
    snapshot.phase === "available" ||
    snapshot.phase === "downloading" ||
    snapshot.phase === "restart-required"
  );
}

export function SidebarUpdateFooter({
  update,
  onOpenWhatsNew,
  onDismissUpdate,
}: {
  update?: InstalledUpdate | null;
  onOpenWhatsNew?: (version: string) => void;
  onDismissUpdate?: () => void;
}) {
  const [snapshot, setSnapshot] = useState<UpdaterSnapshot>({
    phase: "idle",
    currentVersion: "…",
  });
  // "Later" for an installed update: hide the sidebar row for this version
  // until a newer one arrives. The install stays staged — Settings still
  // offers "Restart now" and a manual check re-offers the restart dialog.
  const [dismissedRestart, setDismissedRestart] = useState<string | null>(null);

  // The automatic probe runs on mount whether or not it ends up rendering
  // anything, so a newly published version still surfaces on its own. The
  // snapshot lives here rather than in SidebarUpdate so the footer can drop its
  // padding entirely when neither child has anything to show.
  useEffect(() => {
    let cancelled = false;

    (async () => {
      const currentVersion = await readAppVersion();
      if (cancelled) return;
      const deferred = getPendingRestartVersion();
      if (deferred) {
        setSnapshot({
          phase: "restart-required",
          currentVersion,
          availableVersion: deferred,
        });
        return;
      }
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

  // An install can finish via Settings or the app menu while the footer is
  // mounted. Without this the row keeps reading "Update to X" until clicked
  // (the click self-heals via installPendingUpdate's pending-restart branch).
  useEffect(() => {
    let cancelled = false;
    const unsubscribe = subscribePendingRestart(() => {
      void (async () => {
        const currentVersion = await readAppVersion();
        if (cancelled) return;
        const deferred = getPendingRestartVersion();
        if (!deferred) return;
        setSnapshot((prev) => {
          if (
            prev.phase === "restart-required" &&
            prev.availableVersion === deferred
          ) {
            return prev;
          }
          return {
            phase: "restart-required",
            currentVersion,
            availableVersion: deferred,
          };
        });
      })();
    });
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, []);

  const card =
    update && onOpenWhatsNew && onDismissUpdate ? (
      <UpdateRailCard
        update={update}
        onOpen={onOpenWhatsNew}
        onDismiss={onDismissUpdate}
      />
    ) : null;
  const actionable = isSidebarUpdateActionable(snapshot);
  const restartDismissed =
    snapshot.phase === "restart-required" &&
    dismissedRestart != null &&
    snapshot.availableVersion === dismissedRestart;
  const showUpdate = actionable && !restartDismissed;

  if (!card && !showUpdate) return null;

  // The gap down to the Settings block belongs to that block's own padding, so
  // the footer can disappear without leaving the sidebar's bottom row flush
  // against the scrolling list above it.
  return (
    <div className="flex flex-col gap-1.5 p-2 pb-0">
      {card}
      {showUpdate ? (
        <SidebarUpdate
          snapshot={snapshot}
          onSnapshot={setSnapshot}
          onDismiss={
            snapshot.phase === "restart-required" && snapshot.availableVersion
              ? () => setDismissedRestart(snapshot.availableVersion ?? null)
              : undefined
          }
        />
      ) : null}
    </div>
  );
}

export function SidebarUpdate({
  snapshot,
  onSnapshot,
  onDismiss,
}: {
  snapshot: UpdaterSnapshot;
  onSnapshot: (next: UpdaterSnapshot) => void;
  onDismiss?: () => void;
}) {
  const busy = snapshot.phase === "downloading";
  const needsRestart = snapshot.phase === "restart-required";
  // `busy` only flips after installPendingUpdate awaits readAppVersion, so a
  // second click can still land. The ref closes that window immediately.
  const installing = useRef(false);

  const onClick = useCallback(async () => {
    if (busy || installing.current) return;
    installing.current = true;
    try {
      if (needsRestart) {
        // On relaunch success onProgress is never called (the process is
        // expected to exit); applying the snapshot keeps the button from
        // sticking on "Restart to update" if relaunch ever no-ops.
        onSnapshot(await restartToApplyUpdate(onSnapshot));
        return;
      }
      onSnapshot(await installPendingUpdate(onSnapshot));
    } catch {
      // Helpers are non-throwing (errors surface via dialogs/snapshots);
      // swallow so React handlers never produce unhandled rejections.
    } finally {
      installing.current = false;
    }
  }, [busy, needsRestart, onSnapshot]);

  const label = busy
    ? `Downloading${snapshot.progress != null ? ` ${snapshot.progress}%` : "…"}`
    : needsRestart
      ? `Restart to update to ${snapshot.availableVersion}`
      : `Update to ${snapshot.availableVersion}`;

  return (
    <div className="relative">
      <button
        type="button"
        onClick={onClick}
        disabled={busy}
        className={`flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left transition-colors ${
          needsRestart ? "pr-8" : ""
        } ${
          busy
            ? "bg-content/5 text-content/75 hover:bg-content/10 hover:text-content"
            : "bg-accent/15 text-content hover:bg-accent/20"
        } disabled:cursor-default disabled:opacity-70`}
      >
        <span className="grid size-[18px] shrink-0 place-items-center">
          {busy ? (
            <Loader className="size-4 animate-spin opacity-70" aria-hidden />
          ) : (
            <ArrowDownCircle className="size-4 text-accent" aria-hidden />
          )}
        </span>
        <span className="min-w-0 flex-1 flex items-center">
          <span className="block truncate text-[12px] font-medium leading-tight">
            {label}
          </span>
          <span className="ml-auto block text-[11px] text-content/40">
            v{snapshot.currentVersion}
          </span>
        </span>
      </button>
      {needsRestart && onDismiss ? (
        <button
          type="button"
          aria-label="Dismiss restart notification"
          onClick={onDismiss}
          className="absolute right-1 top-1/2 grid size-6 -translate-y-1/2 place-items-center rounded-md text-content/45 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          <X className="size-3.5" strokeWidth={1.75} />
        </button>
      ) : null}
    </div>
  );
}
