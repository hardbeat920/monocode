import { useCallback, useEffect, useRef, useState } from "react";
import {
  probeForUpdate,
  readAppVersion,
  runUpdateFlow,
  type UpdaterSnapshot,
} from "../lib/updater";

export function VersionUpdate() {
  const inFlight = useRef(false);
  const [snapshot, setSnapshot] = useState<UpdaterSnapshot>({
    phase: "checking",
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
    if (
      inFlight.current ||
      snapshot.phase === "downloading" ||
      snapshot.phase === "checking"
    ) {
      return;
    }

    inFlight.current = true;
    try {
      await runUpdateFlow(true, setSnapshot);
    } finally {
      inFlight.current = false;
    }
  }, [snapshot.phase]);

  return <VersionUpdateButton snapshot={snapshot} onClick={onClick} />;
}

export function VersionUpdateButton({
  snapshot,
  onClick,
}: {
  snapshot: UpdaterSnapshot;
  onClick: () => void;
}) {
  const busy =
    snapshot.phase === "checking" || snapshot.phase === "downloading";
  const available = snapshot.phase === "available";
  let action = "Check for updates";
  if (snapshot.phase === "checking") action = "Checking for updates…";
  if (snapshot.phase === "downloading") {
    action =
      snapshot.progress == null
        ? "Downloading update…"
        : `Downloading update: ${snapshot.progress}%`;
  }
  if (available)
    action = `Update ${snapshot.availableVersion} available. Check for updates`;

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      aria-busy={busy}
      aria-label={`MonoCode ${snapshot.currentVersion}. ${action}`}
      title={action}
      className={`shrink-0 rounded px-1 py-0.5 font-mono text-[10px] tabular-nums hover:bg-content/10 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-default disabled:opacity-50 ${available ? "text-accent" : "text-content/40"}`}
    >
      v{snapshot.currentVersion}
    </button>
  );
}
