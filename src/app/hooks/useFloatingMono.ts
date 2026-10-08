import { useEffect, useRef, useSyncExternalStore } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { IS_MAC } from "../../platform/tauri/platform";
import type { Session } from "../../features/sessions/model/session";
import { findMono } from "../../features/monos/model/mono";
import {
  loadMonoMenuBarIcon,
  subscribeMonoMenuBarIcon,
} from "../../features/settings/model/settings";
import {
  deliverFloatingMonoRequest,
  floatingMonoRoster,
  floatingMonoMenuMascots,
  floatingMonoSession,
  FLOATING_MONO_REQUEST,
  type FloatingMonoHost,
  type FloatingMonoRequest,
} from "../../features/monos/model/floatingMono";

type Host = Omit<FloatingMonoHost, "create"> & {
  /** Add a Mono to the roster and return its id. */
  create(): string;
};

/** The floating webview never boots providers or persists the live transcript. */
export function useFloatingMono(
  sessions: Session[],
  rosterKey: string,
  enabled: boolean,
  host: Host,
) {
  const current = useRef({ sessions, enabled, host });
  current.current = { sessions, enabled, host };
  const active = useRef(new Set<string>());
  const syncing = useRef(Promise.resolve());
  const refresh = useRef<() => void>(() => {});
  const publish = useRef<() => void>(() => {});

  useEffect(() => {
    if (!IS_MAC || !isTauri()) return;
    let disposed = false;
    let draining = false;
    let again = false;
    let unlisten: (() => void) | undefined;
    const lastSessions = new Map<string, Session>();
    const publishSession = (monoId: string, fallback?: Session) => {
      const sessionId = findMono(monoId)?.sessionId;
      const session =
        current.current.sessions.find((s) => s.id === sessionId) ?? fallback;
      if (!session) return;
      lastSessions.set(monoId, session);
      void invoke("mono_chat_publish", {
        monoId,
        session: floatingMonoSession(session),
      }).catch(console.error);
    };
    publish.current = () => {
      for (const monoId of active.current) {
        const mono = findMono(monoId);
        if (!mono || !current.current.enabled) {
          active.current.delete(monoId);
          lastSessions.delete(monoId);
          continue;
        }
        const session = current.current.sessions.find(
          (s) => s.id === mono.sessionId,
        );
        if (session && session !== lastSessions.get(monoId)) {
          publishSession(monoId);
        }
      }
    };
    const drain = async () => {
      again = true;
      if (draining || disposed) return;
      draining = true;
      try {
        while (again && !disposed) {
          again = false;
          const requests =
            await invoke<FloatingMonoRequest[]>("mono_chat_take");
          for (const request of requests) {
            if (disposed) break;
            let error: string | null = null;
            try {
              const session = await deliverFloatingMonoRequest(
                request,
                { ...current.current.host, create },
                () =>
                  disposed
                    ? Promise.resolve(false)
                    : invoke<boolean>("mono_chat_accept", { id: request.id }),
              );
              if (session) {
                active.current.add(request.monoId);
                publishSession(request.monoId, session);
              }
            } catch (reason) {
              error = reason instanceof Error ? reason.message : String(reason);
            }
            await invoke("mono_chat_reply", { id: request.id, error });
          }
        }
      } finally {
        draining = false;
      }
    };
    const roster = () => {
      const monos = floatingMonoRoster(current.current.enabled);
      const hosted = monos.flatMap((mono) => {
        const session = current.current.sessions.find(
          (s) => s.id === mono.sessionId,
        );
        return session ? [{ monoId: mono.id, busy: !!session.busy }] : [];
      });
      return { monos, hosted, mascots: floatingMonoMenuMascots(monos) };
    };
    // The native side only opens Monos it has heard of. This runs inside a
    // drain, so it must not wait on the sync queue, which waits on the drain.
    const create = async (from: string) => {
      const to = current.current.host.create();
      await invoke("mono_chat_sync", roster());
      await invoke("mono_chat_switch", { from, to });
    };
    const sync = () => {
      const args = roster();
      syncing.current = syncing.current
        .catch(() => undefined)
        .then(async () => {
          if (disposed) return;
          await invoke("mono_chat_sync", args);
          await drain();
          publish.current();
        })
        .catch(console.error);
    };
    void getCurrentWebviewWindow()
      .listen(FLOATING_MONO_REQUEST, () => {
        void drain().catch(console.error);
      })
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        refresh.current = sync;
        sync();
      })
      .catch(console.error);
    return () => {
      disposed = true;
      refresh.current = () => {};
      publish.current = () => {};
      unlisten?.();
    };
  }, []);

  const roster = floatingMonoRoster(enabled);
  const hostedKey = sessions
    .filter((s) => roster.some((m) => m.sessionId === s.id))
    .map((s) => `${s.id}:${!!s.busy}`)
    .join("|");
  useEffect(() => {
    refresh.current();
  }, [rosterKey, enabled, hostedKey]);

  // The native side remembers this too; syncing here keeps it matching the
  // setting if the two ever drift.
  const menuBarIcon = useSyncExternalStore(
    subscribeMonoMenuBarIcon,
    loadMonoMenuBarIcon,
  );
  useEffect(() => {
    if (!IS_MAC || !isTauri()) return;
    void invoke("mono_menu_bar_set_visible", { visible: menuBarIcon }).catch(
      console.error,
    );
  }, [menuBarIcon]);

  // Follow the same React commits as the main transcript. A polling interval
  // batches streamed lines into visible jumps and delays the send entrance.
  // This also keeps publishing when a hidden owner has no animation frames.
  useEffect(() => {
    publish.current();
  }, [sessions]);
}
