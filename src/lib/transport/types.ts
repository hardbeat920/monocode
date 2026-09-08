/** Transport seam between the React UI and the command backend.
 *
 * MERGE NOTE: additive-only. Desktop default path (LocalTransport) calls the
 * exact same Tauri APIs as before, so upstream updates to call sites keep
 * working unchanged.
 */

/** Matches `@tauri-apps/api/event` UnlistenFn without importing Tauri here. */
export type UnlistenFn = () => void;

export type TransportMode = "local" | "remote";

/** Same envelope shape Tauri event listeners already receive (`event.payload`). */
export interface TransportEnvelope<T> {
  payload: T;
}

export type TransportEventHandler<T> = (event: TransportEnvelope<T>) => void;

export interface Transport {
  readonly mode: TransportMode;
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(
    event: string,
    handler: TransportEventHandler<T>,
  ): Promise<UnlistenFn>;
  dispose(): void;
}
