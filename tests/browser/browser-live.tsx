// "Show in MonoCode" prototype: the real workspace, with the browser module's
// commands answered by the Playwright test, which drives a real Chrome over
// CDP (`window.liveBridge`). Events reach `listen` through `__emitTauri`;
// the pane asks for pictures with `browser_live_frame`, which the bridge
// answers with the picture's fields, packed here as `live::LiveFrame::packed`.
import {
  browserState,
  browserTab,
} from "../../src/features/settings/model/browser.fixtures";

type Internals = {
  invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  transformCallback: (callback: (event: unknown) => void) => number;
  [key: string]: unknown;
};

const params = new URLSearchParams(location.search);
const SESSION = "workspace-session-0";
const state = browserState({
  sessions: [
    {
      session_id: SESSION,
      window: "main",
      granted: true,
      turn_active: false,
      tabs: 1,
      requested: false,
      turned_off: false,
    },
  ],
  tabs: [
    browserTab({
      tab: "t_live0001",
      session_id: SESSION,
      url: params.get("url") ?? "http://127.0.0.1/checkout",
      title: params.get("title") ?? "Checkout",
    }),
  ],
});
const callbacks = new Map<number, (event: unknown) => void>();
const listeners = new Map<string, number[]>();
let nextCallback = 1;
const bridge = (command: string, args: unknown) =>
  (
    window as unknown as {
      liveBridge: (command: string, args: unknown) => Promise<unknown>;
    }
  ).liveBridge(command, args);

let internals: Internals | undefined;
Object.defineProperty(window, "__TAURI_INTERNALS__", {
  configurable: true,
  get: () => internals,
  set(value: Internals) {
    internals = {
      ...value,
      transformCallback(callback) {
        const id = nextCallback++;
        callbacks.set(id, callback);
        return id;
      },
      async invoke(command, args = {}) {
        if (command === "plugin:event|listen") {
          const event = String(args.event);
          listeners.set(event, [
            ...(listeners.get(event) ?? []),
            Number(args.handler),
          ]);
          return Number(args.handler);
        }
        if (command === "plugin:event|unlisten") return;
        if (command === "browser_status") return state;
        if (command === "browser_live_frame") {
          const picture = (await bridge(command, args)) as Picture | null;
          return picture ? pack(picture) : new ArrayBuffer(0);
        }
        if (command.startsWith("browser_live_")) return bridge(command, args);
        if (command.startsWith("browser_") || command === "dev_servers")
          return [];
        return value.invoke(command, args);
      },
    };
  },
});
type Picture = {
  format: string;
  seq: number;
  width: number;
  height: number;
  scrollX?: number;
  scrollY?: number;
  data: string;
};

/** A picture as `live::LiveFrame::packed` lays it out. */
function pack(frame: Picture): ArrayBuffer {
  const picture = Uint8Array.from(atob(frame.data), (c) => c.charCodeAt(0));
  const buffer = new ArrayBuffer(40 + picture.length);
  const head = new DataView(buffer);
  head.setUint8(0, frame.format === "png" ? 1 : 0);
  head.setUint32(4, frame.seq, true);
  head.setFloat64(8, frame.width, true);
  head.setFloat64(16, frame.height, true);
  head.setFloat64(24, frame.scrollX ?? 0, true);
  head.setFloat64(32, frame.scrollY ?? 0, true);
  new Uint8Array(buffer, 40).set(picture);
  return buffer;
}

Object.assign(window, {
  // The browser store starts only inside Tauri.
  isTauri: true,
  __emitTauri(event: string, payload: unknown) {
    for (const id of listeners.get(event) ?? [])
      callbacks.get(id)?.({ event, id, payload });
  },
});

await import("./workspace-performance");
