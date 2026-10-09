import { parseQuickLaunch, type QuickLaunch } from "./quickComposer";
import { ProjectNotFoundError } from "../../projects/model/projectLocationError";

const INITIAL_RETRY_MS = 250;
const MAX_RETRY_MS = 30_000;

class InvalidLaunchError extends Error {}

/** Serialize mount/focus/event deliveries and acknowledge only accepted launches.
 * Receipts survive effect remounts so a failed ACK never starts a second session.
 * Transient failures retry even when a hidden workspace gets no further events. */
export function launchReceiver(options: {
  take: () => Promise<unknown>;
  accept: (launch: QuickLaunch, id: string) => Promise<void>;
  ack: (id: string) => Promise<void>;
  disposed: () => boolean;
  accepted: Set<string>;
  accepting: Map<string, Promise<void>>;
}) {
  let running: Promise<void> | undefined;
  let requested = false;
  let stopped = false;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let retryDelay = INITIAL_RETRY_MS;
  const disposed = () => stopped || options.disposed();
  const cancelRetry = () => {
    clearTimeout(retryTimer);
    retryTimer = undefined;
  };

  function receive(): Promise<void> {
    if (disposed()) return Promise.resolve();
    requested = true;
    if (running) return running;
    // An external event can retry immediately; do not leave a second timer alive.
    cancelRetry();
    running = (async () => {
      do {
        requested = false;
        while (!disposed()) {
          const value = await options.take();
          if (disposed()) break;
          if (value == null) {
            retryDelay = INITIAL_RETRY_MS;
            break;
          }
          const envelope = value as { id?: unknown; request?: unknown };
          const launch = parseQuickLaunch(envelope.request);
          if (typeof envelope.id !== "string" || !envelope.id || !launch)
            throw new InvalidLaunchError(
              "Invalid queued session; retained without automatic retry.",
            );
          if (!options.accepted.has(envelope.id)) {
            const id = envelope.id;
            let acceptance = options.accepting.get(id);
            if (!acceptance) {
              acceptance = options
                .accept(launch, id)
                .then(() => {
                  options.accepted.add(id);
                })
                .finally(() => {
                  options.accepting.delete(id);
                });
              options.accepting.set(id, acceptance);
            }
            await acceptance;
          }
          if (disposed()) break;
          await options.ack(envelope.id);
          options.accepted.delete(envelope.id);
          retryDelay = INITIAL_RETRY_MS;
        }
      } while (requested && !disposed());
    })()
      .catch((error: unknown) => {
        // Missing projects stay queued until an external receive retries them
        // after reconnection; timers must not keep appending error blocks.
        if (
          !disposed() &&
          !(error instanceof InvalidLaunchError) &&
          !(error instanceof ProjectNotFoundError)
        ) {
          retryTimer = setTimeout(() => {
            retryTimer = undefined;
            // receive schedules the next retry on failure. Consume the rejection
            // here because timer attempts have no event caller awaiting them.
            void receive().catch(() => undefined);
          }, retryDelay);
          retryDelay = Math.min(retryDelay * 2, MAX_RETRY_MS);
        }
        throw error;
      })
      .finally(() => {
        running = undefined;
      });
    return running;
  }

  return Object.assign(receive, {
    dispose() {
      stopped = true;
      cancelRetry();
    },
  });
}
