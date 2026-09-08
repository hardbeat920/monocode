import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type {
  Transport,
  TransportEventHandler,
  UnlistenFn,
} from "./types";

/** Desktop default: direct in-process Tauri calls, byte-identical to before. */
export class LocalTransport implements Transport {
  readonly mode = "local" as const;

  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return tauriInvoke<T>(command, args);
  }

  async listen<T>(
    event: string,
    handler: TransportEventHandler<T>,
  ): Promise<UnlistenFn> {
    return tauriListen<T>(event, (tauriEvent) => {
      handler({ payload: tauriEvent.payload });
    });
  }

  dispose(): void {
    // Nothing to tear down; window-owned listeners clean themselves up.
  }
}
