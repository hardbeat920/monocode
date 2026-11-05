import { ArrowDownCircle, Loader } from "../../shared/ui/icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  installPendingUpdate,
  probeForUpdateThrottled,
  readAppVersion,
  type UpdateCheckKind,
  type UpdaterSnapshot,
} from "../model/updater";
import type { InstalledUpdate } from "../model/updateNotice";
import { UpdateRailCard } from "./UpdateRailCard";

// The sidebar row only earns its space when there is something to act on: an
// update waiting to be installed, or one already downloading. Every other phase
// — including a probe that failed — stays silent, because manual "Check for
// updates" already lives in Settings and the app menu.
export function isSidebarUpdateActionable(snapshot: UpdaterSnapshot): boolean {
  return snapshot.phase === "available" || snapshot.phase === "downloading";
}

const BACKGROUND_PROBE_TICK_MS = 15 * 60 * 1000;

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

  // Automatic probes run on focus and on a timer (both throttled in
  // probeForUpdateThrottled), whether or not they end up rendering anything, so
  // a newly published version still surfaces on its own. The snapshot lives
  // here rather than in SidebarUpdate so the footer can drop its padding
  // entirely when neither child has anything to show.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const probe = async (kind: UpdateCheckKind) => {
      let update: Awaited<ReturnType<typeof probeForUpdateThrottled>>;
      try {
        update = await probeForUpdateThrottled(kind);
      } catch {
        return;
      }
      if (update === undefined) return;
      const currentVersion = await readAppVersion();
      if (cancelled) return;
      const next: UpdaterSnapshot = update
        ? { phase: "available", currentVersion, availableVersion: update.version }
        : { phase: "current", currentVersion };
      // A download started from the sidebar owns the row until it finishes.
      setSnapshot((prev) => (prev.phase === "downloading" ? prev : next));
    };

    void probe(document.hasFocus() ? "fg" : "bg");
    // Ticks often and lets the throttle decide, so timer drift never skips a
    // whole interval.
    const timer = setInterval(() => void probe("bg"), BACKGROUND_PROBE_TICK_MS);
    // getCurrentWindow throws outside the Tauri runtime (tests, plain browser);
    // the timer still covers background checks there.
    try {
      void getCurrentWindow()
        .onFocusChanged(({ payload: focused }) => {
          if (focused) void probe("fg");
        })
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // No window to listen to.
    }

    return () => {
      cancelled = true;
      clearInterval(timer);
      unlisten?.();
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

  if (!card && !actionable) return null;

  // The gap down to the Settings block belongs to that block's own padding, so
  // the footer can disappear without leaving the sidebar's bottom row flush
  // against the scrolling list above it.
  return (
    <div className="flex flex-col gap-1.5 p-2 pb-0">
      {card}
      {actionable ? (
        <SidebarUpdate snapshot={snapshot} onSnapshot={setSnapshot} />
      ) : null}
    </div>
  );
}

export function SidebarUpdate({
  snapshot,
  onSnapshot,
}: {
  snapshot: UpdaterSnapshot;
  onSnapshot: (next: UpdaterSnapshot) => void;
}) {
  const busy = snapshot.phase === "downloading";
  // `busy` only flips after installPendingUpdate awaits readAppVersion, so a
  // second click can still land. The ref closes that window immediately.
  const installing = useRef(false);

  const onClick = useCallback(async () => {
    if (busy || installing.current) return;
    installing.current = true;
    try {
      await installPendingUpdate(onSnapshot);
    } finally {
      installing.current = false;
    }
  }, [busy, onSnapshot]);

  const label = busy
    ? `Downloading${snapshot.progress != null ? ` ${snapshot.progress}%` : "…"}`
    : `Update to ${snapshot.availableVersion}`;

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      className={`flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left transition-colors ${
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
  );
}
