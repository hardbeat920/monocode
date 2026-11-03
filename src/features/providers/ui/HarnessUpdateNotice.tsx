import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { createPortal } from "react-dom";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { LAYER } from "../../../shared/lib/layers";
import { Check, Loader, X } from "../../../shared/ui/icons";
import { isPickerProviderVisible } from "../../sessions/model/models";
import { HARNESS_TITLE, type HarnessId } from "../../sessions/model/session";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import {
  checkInstalledHarnessVersions,
  getHarnessUpdateSnapshot,
  runHarnessUpdate,
  subscribeHarnessUpdates,
  type HarnessUpdateRun,
} from "../model/harnessUpdateActions";
import {
  claimLaunchHarnessUpdateCheck,
  onHarnessUpdated,
  pendingHarnessUpdates,
  type HarnessUpdate,
} from "../model/harnessUpdates";

/**
 * Shared by every mount in this window: a StrictMode remount must reuse the
 * first run, since the launch claim it made cannot be taken twice.
 */
let launchCheck: Promise<HarnessUpdate[]> | null = null;

function checkForHarnessUpdates(): Promise<HarnessUpdate[]> {
  launchCheck ??= runLaunchCheck().catch(() => []);
  return launchCheck;
}

/**
 * Unprompted, so a harness whose lookup failed is left out silently. Every
 * installed harness is checked so Settings can list them all, but the toast
 * offers only those shown in the model picker.
 */
async function runLaunchCheck(): Promise<HarnessUpdate[]> {
  if (!(await claimLaunchHarnessUpdateCheck())) return [];
  return pendingHarnessUpdates(await checkInstalledHarnessVersions()).filter(
    (update) => isPickerProviderVisible(update.harness),
  );
}

/** Checked once per app launch, in whichever window asks first. */
export function HarnessUpdateNotice({
  topOffset = 12,
  onHeightChange,
}: {
  topOffset?: number;
  onHeightChange?: (height: number) => void;
}) {
  const panelRef = useRef<HTMLElement>(null);
  const [updates, setUpdates] = useState<HarnessUpdate[]>([]);
  const { runs } = useSyncExternalStore(
    subscribeHarnessUpdates,
    getHarnessUpdateSnapshot,
    getHarnessUpdateSnapshot,
  );

  // Mounted in every window, so the window that ran the update tells the
  // others to pick up the new CLI's models too.
  useEffect(() => {
    const unlisten = onHarnessUpdated((harness) => {
      void refreshHarnessCatalogs([harness], { force: true });
    }).catch(() => undefined);
    return () => {
      void unlisten.then((stop) => stop?.());
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void checkForHarnessUpdates().then((next) => {
      if (!cancelled) setUpdates(next);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const visible = updates.length > 0;
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel || !onHeightChange) return;
    const measure = () => onHeightChange(panel.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    return () => {
      observer.disconnect();
      onHeightChange(0);
    };
  }, [visible, onHeightChange]);
  if (!visible) return null;

  const stateOf = (harness: HarnessId): HarnessUpdateRun =>
    runs[harness] ?? { status: "idle" };
  const busy = updates.some(
    (update) => stateOf(update.harness).status === "updating",
  );
  const pending = updates.filter((update) => {
    const status = stateOf(update.harness).status;
    return status === "idle" || status === "failed";
  });
  const anyUpdated = updates.some(
    (update) => stateOf(update.harness).status === "updated",
  );

  const start = (targets: HarnessUpdate[]) => {
    for (const update of targets) void runHarnessUpdate(update);
  };
  const dismiss = () => {
    // Only for this run: the next launch checks and offers again.
    launchCheck = Promise.resolve([]);
    setUpdates([]);
  };

  return createPortal(
    <section
      ref={panelRef}
      aria-label="Harness updates"
      role="status"
      style={{ zIndex: LAYER.toast, top: topOffset }}
      className="fixed right-3 isolate w-[min(340px,calc(100vw-24px))] overflow-hidden rounded-xl border border-content/10 text-content shadow-xl"
    >
      <GlassBackdrop />
      <div className="relative z-[1]">
        <div className="flex items-center gap-2 border-b border-stroke px-3 py-2">
          <span className="min-w-0 flex-1 truncate text-[12px] font-semibold">
            {updates.length === 1
              ? "Harness update available"
              : "Harness updates available"}
          </span>
          {pending.length > 1 ? (
            <button
              type="button"
              className="rounded-md px-2 py-1 text-[11px] font-medium text-content/70 hover:bg-content/10 hover:text-content"
              onClick={() => start(pending)}
            >
              Update all
            </button>
          ) : null}
          <button
            type="button"
            aria-label="Dismiss harness updates"
            disabled={busy}
            className="grid size-6 shrink-0 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content disabled:opacity-40 disabled:hover:bg-transparent"
            onClick={dismiss}
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        </div>
        <div className="divide-y divide-stroke">
          {updates.map((update) => (
            <HarnessUpdateRow
              key={update.harness}
              update={update}
              state={stateOf(update.harness)}
              onUpdate={() => start([update])}
            />
          ))}
        </div>
        <p className="border-t border-stroke px-3 py-2 text-[11px] text-content/50">
          {anyUpdated
            ? "Model picker refreshed with the new version’s models."
            : "New models often need the latest version."}
        </p>
      </div>
    </section>,
    document.body,
  );
}

function HarnessUpdateRow({
  update,
  state,
  onUpdate,
}: {
  update: HarnessUpdate;
  state: HarnessUpdateRun;
  onUpdate: () => void;
}) {
  return (
    <article className="px-3 py-2.5">
      <div className="flex items-center gap-2">
        <HarnessIcon harness={update.harness} className="size-4 shrink-0" />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">
          {HARNESS_TITLE[update.harness]}
        </span>
        {state.status === "updated" ? (
          <span className="flex shrink-0 items-center gap-1 text-[11px] text-emerald-400">
            <Check className="size-3.5" />
            Updated to {state.version}
          </span>
        ) : (
          <>
            <span className="shrink-0 font-mono text-[11px] text-content/50">
              {update.installed} → {update.latest}
            </span>
            <button
              type="button"
              disabled={state.status === "updating"}
              className="inline-flex shrink-0 items-center gap-1.5 rounded-md bg-content/10 px-2 py-1 text-[11px] font-medium hover:bg-content/15 disabled:opacity-60 disabled:hover:bg-content/10"
              onClick={onUpdate}
            >
              {state.status === "updating" ? (
                <>
                  <Loader className="size-3 animate-spin" />
                  Updating
                </>
              ) : state.status === "failed" ? (
                "Retry"
              ) : (
                "Update"
              )}
            </button>
          </>
        )}
      </div>
      {state.status === "failed" ? (
        <p
          role="alert"
          className="mt-1.5 line-clamp-2 text-[11px] leading-relaxed text-red-300/90"
          title={state.error}
        >
          {state.error}
        </p>
      ) : null}
    </article>
  );
}
