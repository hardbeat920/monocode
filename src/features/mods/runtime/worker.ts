/**
 * The runtime inside a mod's worker: `on`, the handler chain, `$`, atoms, the
 * jsx factory and callback ids. The loader bundles this file's source with
 * the mod, so it may only use type imports and globals a worker has.
 */
import type {
  Atom,
  Engine,
  HostToWorker,
  Json,
  ModEventName,
  Node,
  Register,
  RpcMethod,
  SessionInfo,
  ToolCallResult,
  UserConfigValues,
  WorkerToHost,
} from "../api/types";

type AnyHandler = (
  $: Engine,
  event: unknown,
  next: (event?: unknown) => Promise<unknown>,
) => unknown;

/** Element tags; JSX resolves `<Box>` to these through the `monocode` shim. */
export const Box = "Box";
export const Text = "Text";
export const Button = "Button";
export const Input = "Input";
export const Badge = "Badge";
export const Dot = "Dot";
export const Icon = "Icon";
export const Spinner = "Spinner";
export const Skeleton = "Skeleton";
export const Markdown = "Markdown";
export const Tree = "Tree";
export const Fragment = "Fragment";

const WITH_CHILDREN = new Set(["Box", "Text"]);

function flatten(children: unknown): Node[] {
  if (Array.isArray(children)) return children.flatMap(flatten);
  return children === undefined ? [] : [children as Node];
}

/** The automatic-runtime factory: builds `Element` data, never DOM. */
export function jsx(
  type: string | ((props: Record<string, unknown>) => unknown),
  props: Record<string, unknown>,
  key?: string,
): unknown {
  if (typeof type === "function") return type(props ?? {});
  const { children, ...rest } = props ?? {};
  if (type === Fragment) return flatten(children);
  if (key !== undefined) rest.key = key;
  return WITH_CHILDREN.has(type)
    ? { type, props: rest, children: flatten(children) }
    : { type, props: rest };
}
export const jsxs = jsx;

/** In-memory atoms for the `claude-code` import shim; `$.state` uses the same store. */
const atoms = new Map<string, unknown>();
let onAtomWrite = () => {};

export function atom<T>(key: string, initial: T & Json): Atom<T> {
  if (!atoms.has(key)) atoms.set(key, initial);
  return { key, initial };
}

export function read<T>(a: Atom<T>): T {
  return (atoms.has(a.key) ? atoms.get(a.key) : a.initial) as T;
}

export function update<T>(a: Atom<T>, fn: (value: T) => T): void {
  atoms.set(a.key, fn(read(a)));
  onAtomWrite();
}

function matches(filter: unknown, event: unknown): boolean {
  if (!filter || typeof filter !== "object") return true;
  const fields = event as Record<string, unknown>;
  return Object.entries(filter).every(
    ([key, value]) => value === undefined || fields?.[key] === value,
  );
}

/** Wire form of a tree: functions become `{ $cb }` ids owned by `view`. */
function toWire(
  node: unknown,
  register: (fn: (...args: unknown[]) => void) => string,
): unknown {
  if (typeof node === "function") return { $cb: register(node as never) };
  if (Array.isArray(node)) return node.map((n) => toWire(n, register));
  if (!node || typeof node !== "object") return node;
  return Object.fromEntries(
    Object.entries(node).map(([k, v]) => [k, toWire(v, register)]),
  );
}

export function createRuntime(post: (message: WorkerToHost) => void) {
  const handlers: {
    name: ModEventName;
    filter: unknown;
    handler: AnyHandler;
  }[] = [];
  const calls = new Map<
    number,
    { resolve(v: unknown): void; reject(e: Error): void }
  >();
  const callbacks = new Map<string, (...args: unknown[]) => void>();
  const viewCallbacks = new Map<string, string[]>();
  const toolWaits = new Map<string, (result: ToolCallResult) => void>();
  /** Results that came before the handler reached `next`, by callId, while it runs. */
  const toolDone = new Map<string, ToolCallResult | undefined>();
  let nextCall = 1;
  let nextCb = 1;
  let invalidateQueued = false;
  let config: UserConfigValues = {};

  const call = <T>(method: RpcMethod, args: unknown[]): Promise<T> =>
    new Promise<T>((resolve, reject) => {
      const id = nextCall++;
      calls.set(id, { resolve: resolve as (v: unknown) => void, reject });
      post({ kind: "call", id, method, args });
    });
  const effect = (method: string, args: unknown[]) =>
    post({ kind: "effect", method, args });
  // Several writes in one tick re-render once.
  const invalidate = () => {
    if (invalidateQueued) return;
    invalidateQueued = true;
    queueMicrotask(() => {
      invalidateQueued = false;
      effect("ui.invalidate", []);
    });
  };
  onAtomWrite = invalidate;

  const timer = (id: ReturnType<typeof setTimeout>, interval: boolean) => ({
    cancel: () => (interval ? clearInterval(id) : clearTimeout(id)),
  });

  const engine = (sessionId?: string): Engine => ({
    process: {
      run: (argv, options) => call("process.run", [argv, options ?? {}]),
    },
    fs: {
      read: (path) => call("fs.read", [path]),
      list: (dir) => call("fs.list", [dir]),
      stat: (path) => call("fs.stat", [path]),
    },
    http: {
      request: (url, options) => call("http.request", [url, options ?? {}]),
    },
    store: {
      // Rust sends a missing key as null; the API promises undefined.
      get: async (key) => (await call("store.get", [key])) ?? undefined,
      set: async (key, value) => {
        await call("store.set", [key, value]);
        invalidate();
      },
      delete: async (key) => {
        await call("store.delete", [key]);
        invalidate();
      },
    },
    state: { atom, read, update },
    clock: {
      now: () => Date.now(),
      every: (ms, fn) => timer(setInterval(fn, ms), true),
      after: (ms, fn) => timer(setTimeout(fn, ms), false),
    },
    config,
    env: {
      get: async (name) =>
        (await call<string | null>("env.get", [name])) ?? undefined,
    },
    session: {
      current: () =>
        call<SessionInfo | null>("session.current", [sessionId ?? null]),
      agents: (id) => call("session.agents", [id]),
      toolCalls: (id) => call("session.toolCalls", [id]),
    },
    workspace: {
      projectCwd: () => call("workspace.projectCwd", []),
      isLightScheme: () => call("workspace.isLightScheme", []),
    },
    command: { register: (command) => effect("command.register", [command]) },
    ui: {
      open: (view) => effect("ui.open", [view]),
      close: (view) => effect("ui.close", [view]),
      toast: (text, options) => effect("ui.toast", [text, options ?? {}]),
      status: (id, text, options) =>
        effect("ui.status", [id, text, options ?? {}]),
      invalidate: () => effect("ui.invalidate", []),
      copy: (text) => call("ui.copy", [text]),
      openUrl: (url) => effect("ui.openUrl", [url]),
      openFile: (path, options) => effect("ui.openFile", [path, options ?? {}]),
    },
  });
  /** Runs the matching handlers in order; `next` passes the event on. */
  function dispatch(name: ModEventName, event: unknown): Promise<unknown> {
    const chain = handlers.filter(
      (h) => h.name === name && matches(h.filter, event),
    );
    const fields = event as { sessionId?: string; callId?: string };
    const $ = engine(fields?.sessionId);
    const callId = name === "tool.call" ? fields?.callId : undefined;
    const tail = (): Promise<unknown> => {
      if (!callId) return Promise.resolve(null);
      const early = toolDone.get(callId);
      if (early) return Promise.resolve(early);
      return new Promise((resolve) => toolWaits.set(callId, resolve));
    };
    const run = async (index: number, current: unknown): Promise<unknown> => {
      const entry = chain[index];
      if (!entry) return tail();
      return entry.handler($, current, (e) => run(index + 1, e ?? current));
    };
    if (!callId) return run(0, event);
    toolDone.set(callId, undefined);
    return run(0, event).finally(() => toolDone.delete(callId));
  }

  async function onEvent(id: number, name: ModEventName, event: unknown) {
    try {
      let value = await dispatch(name, event);
      if (name === "ui.render") {
        const view = (event as { requestId: string }).requestId;
        for (const cb of viewCallbacks.get(view) ?? []) callbacks.delete(cb);
        const owned: string[] = [];
        viewCallbacks.set(view, owned);
        value = toWire(value ?? null, (fn) => {
          const cb = `${view}:${nextCb++}`;
          callbacks.set(cb, fn);
          owned.push(cb);
          return cb;
        });
      }
      post({ kind: "result", id, ok: true, value: value ?? null });
    } catch (error) {
      post({ kind: "result", id, ok: false, error: String(error) });
    }
  }

  const on = ((name: ModEventName, filter: unknown, handler: AnyHandler) => {
    handlers.push({ name, filter, handler });
  }) as unknown as Parameters<Register>[0];

  return {
    async load(register: Register, values: UserConfigValues) {
      config = values;
      await register(on, engine());
      post({
        kind: "ready",
        subscriptions: handlers.map(({ name, filter }) => ({ name, filter })),
      });
    },

    receive(message: HostToWorker) {
      switch (message.kind) {
        case "result": {
          const pending = calls.get(message.id);
          calls.delete(message.id);
          if (message.ok) pending?.resolve(message.value);
          else pending?.reject(new Error(message.error));
          return;
        }
        case "event":
          void onEvent(message.id, message.name, message.event);
          return;
        case "tool.done": {
          const wait = toolWaits.get(message.callId);
          toolWaits.delete(message.callId);
          if (wait) wait(message.result);
          else if (toolDone.has(message.callId))
            toolDone.set(message.callId, message.result);
          return;
        }
        case "callback":
          callbacks.get(message.cb)?.(...message.args);
          return;
        default:
          return;
      }
    },
  };
}

/** Bundle entry: waits for `load`, runs `register`, then serves the host. */
export function boot(register: Register | undefined) {
  const scope = self as unknown as {
    postMessage(m: WorkerToHost): void;
    onmessage: ((e: MessageEvent<HostToWorker>) => void) | null;
  };
  const runtime = createRuntime((m) => scope.postMessage(m));
  const log = (level: "info" | "warn" | "error", args: unknown[]) =>
    scope.postMessage({
      kind: "log",
      level,
      message: args.map(String).join(" "),
    });
  console.log = (...args) => log("info", args);
  console.warn = (...args) => log("warn", args);
  console.error = (...args) => log("error", args);
  scope.onmessage = (e) => {
    const message = e.data;
    if (message.kind !== "load") return runtime.receive(message);
    if (typeof register !== "function")
      throw new Error("The mod does not export register");
    runtime.load(register, message.config).catch((error) => {
      // Rethrown outside the promise so the frame's worker.onerror reports a crash.
      setTimeout(() => {
        throw error;
      });
    });
  };
}
