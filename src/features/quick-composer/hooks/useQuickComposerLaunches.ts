import { probeHarnessAvailability } from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type Event } from "@tauri-apps/api/event";
import { loadQuickComposerEnabled } from "../../settings/model/settings";
import { launchReceiver } from "../model/launchDelivery";
import { prepareQuickComposerWhenIdle } from "../model/prepareQuickComposer";
import {
  liveQuickCatalog,
  isHarnessId,
  QUICK_COMPOSER_CATALOG_EVENT,
  QUICK_COMPOSER_CATALOG_REQUEST_EVENT,
  QUICK_COMPOSER_LAUNCH_EVENT,
  quickComposerSupported,
  setQuickComposerShortcut,
  type QuickLaunch,
} from "../model/quickComposer";

/**
 * Claims the global shortcut per the setting, answers the panel's requests for
 * model catalogs, and starts sessions handed to this window. The backend keeps
 * each launch until acceptance is acknowledged; mount, focus, and launch events
 * drain the queue through one serialized receiver.
 */
export function useQuickComposerLaunches(
  onLaunch: (launch: QuickLaunch, id: string) => Promise<void>,
) {
  const accepting = useRef(new Map<string, Promise<void>>());
  const accepted = useRef(new Set<string>());
  const onLaunchRef = useRef(onLaunch);
  onLaunchRef.current = onLaunch;

  useEffect(() => {
    if (!quickComposerSupported()) return;
    const stopPreparing = prepareQuickComposerWhenIdle();
    void setQuickComposerShortcut(loadQuickComposerEnabled()).catch(
      () => undefined,
    );

    let disposed = false;
    const receive = launchReceiver({
      take: () => invoke("quick_composer_take"),
      accept: (launch, id) => onLaunchRef.current(launch, id),
      ack: (id) => invoke("quick_composer_ack", { id }),
      disposed: () => disposed,
      accepted: accepted.current,
      accepting: accepting.current,
    });
    const take = () =>
      receive().catch((error) => {
        console.error(
          "Quick session delivery failed; session remains queued:",
          error,
        );
      });
    const onFocus = () => void take();
    const subscriptions: Array<() => void> = [];
    const subscribe = async (
      event: string,
      handler: (event: Event<unknown>) => void,
    ) => {
      const stop = await listen(event, handler);
      if (disposed) stop();
      else subscriptions.push(stop);
    };
    void Promise.all([
      subscribe(QUICK_COMPOSER_LAUNCH_EVENT, () => void take()),
      subscribe(QUICK_COMPOSER_CATALOG_REQUEST_EVENT, (event) => {
        void probeHarnessAvailability()
          .then(async () => {
            if (disposed) return;
            void emit(QUICK_COMPOSER_CATALOG_EVENT, liveQuickCatalog());
            // Only probe the provider the user opened, like the workspace picker.
            if (isHarnessId(event.payload)) {
              await refreshHarnessCatalogs([event.payload]);
              if (!disposed)
                void emit(QUICK_COMPOSER_CATALOG_EVENT, liveQuickCatalog());
            }
          })
          .catch(() => undefined);
      }),
    ])
      .then(() => take())
      .catch(() => undefined);
    window.addEventListener("focus", onFocus);
    return () => {
      disposed = true;
      receive.dispose();
      stopPreparing();
      for (const stop of subscriptions) stop();
      window.removeEventListener("focus", onFocus);
    };
  }, []);
}
