import { useSyncExternalStore } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  MOD_API_VERSION,
  type Element,
  type ModEventName,
  type ModManifest,
  type RenderProps,
  type RenderSite,
  type RpcMethod,
  type Tone,
  type UserConfigValues,
} from "../api/types";
import type { ModEmission } from "./events";
import { mountFrame, type FrameMessage, type ModFrame } from "./frame";
import {
  addUnpackedMod,
  bundleMod,
  forgetBundle,
  listMods,
  setModEnabled,
  type ModRecord,
} from "./loader";
import { callRpc, type ModHostContext } from "./rpc";

export type ModView = {
  name: string;
  id: string;
  title: string;
  site: "Pane" | "SidebarTab";
  icon?: string;
};

type ModStatus = {
  name: string;
  id: string;
  text: string;
  tone?: Tone;
  icon?: string;
};
type ModToast = {
  key: string;
  name: string;
  text: string;
  tone?: Tone;
  timeoutMs: number;
};

export type ModHost = {
  views(): ModView[];
  status(): ModStatus[];
  toasts(): ModToast[];
  render(
    name: string,
    site: RenderSite,
    requestId: string,
    props: RenderProps,
    sessionId?: string,
  ): Promise<Element | null>;
  /** WireCallback press. */
  invoke(name: string, cb: string, args: unknown[]): void;
  focus(name: string, site: RenderSite, requestId: string): void;
  /** Any of the above changed. */
  subscribe(listener: () => void): () => void;
  /** Bumps when the mod starts or asks its views to render again. */
  renderVersion(name: string): number;
  mods(): {
    manifest: ModManifest;
    enabled: boolean;
    source: ModRecord["source"];
    dir: string;
    /** Folders of same-named mods this one hides. */
    shadows: string[];
    /** Manifest defaults overlaid with the user's values. */
    config: UserConfigValues;
    error?: string;
    crashed?: boolean;
  }[];
  setEnabled(name: string, enabled: boolean): Promise<void>;
  setConfig(name: string, values: UserConfigValues): Promise<void>;
  reload(name: string): Promise<void>;
  loadUnpacked(path: string): Promise<void>;
};

const MAX_IN_FLIGHT = 32;
const RENDER_TIMEOUT_MS = 2000;
const READY_TIMEOUT_MS = 10_000;
const DEFAULT_TOAST_MS = 5000;
const MAX_TOAST_MS = 60_000;
const MAX_TOASTS_PER_MOD = 3;
/** How long after a press a mod may open a URL or file, or copy (like a browser's user activation). */
const ACTIVATION_MS = 5000;
const CONFIG_KEY = "monocode.mods.config.";

/** A running mod: its frame, subscriptions and outstanding requests. */
type Live = {
  frame: ModFrame;
  subscriptions: Set<ModEventName>;
  ready: boolean;
  nextId: number;
  inFlight: number;
  waits: Map<number, (value: unknown) => void>;
  lastTrees: Map<string, Element | null>;
  /** Last user press on one of the mod's views. */
  pressedAt: number;
};

const listeners = new Set<() => void>();
let version = 0;
let ctx: ModHostContext | null = null;
let records: ModRecord[] = [];
const live = new Map<string, Live>();
const crashed = new Map<string, string>();
const renders = new Map<string, number>();
let views: ModView[] = [];
let statuses: ModStatus[] = [];
let toasts: ModToast[] = [];
let nextToast = 1;

function changed() {
  version++;
  for (const listener of listeners) listener();
}

// Keyed by folder, so a same-named mod elsewhere does not get this one's values.
function loadConfig(dir: string): UserConfigValues {
  try {
    return JSON.parse(localStorage.getItem(CONFIG_KEY + dir) ?? "{}");
  } catch {
    return {};
  }
}

/** Manifest defaults overlaid with the user's values. */
function resolvedConfig(record: ModRecord): UserConfigValues {
  const defaults: UserConfigValues = {};
  for (const [key, field] of Object.entries(record.manifest.userConfig ?? {})) {
    if (field.default !== undefined) defaults[key] = field.default;
  }
  return { ...defaults, ...loadConfig(record.dir) };
}

function send(
  name: string,
  eventName: ModEventName,
  event: unknown,
): Promise<unknown> {
  const mod = live.get(name);
  if (!mod?.ready) return Promise.resolve(null);
  const id = mod.nextId++;
  return new Promise((resolve) => {
    mod.waits.set(id, resolve);
    mod.frame.post({ kind: "event", id, name: eventName, event });
  });
}

/** A crashed mod keeps its views so they can show the error. */
function stop(name: string, keepViews = false) {
  statuses = statuses.filter((s) => s.name !== name);
  toasts = toasts.filter((t) => t.name !== name);
  if (!keepViews) views = views.filter((v) => v.name !== name);
  const mod = live.get(name);
  if (!mod) return;
  mod.frame.post({ kind: "unload" });
  mod.frame.destroy();
  for (const resolve of mod.waits.values()) resolve(null);
  live.delete(name);
}

function crash(name: string, message: string) {
  console.error(`[mod ${name}]`, message);
  stop(name, true);
  crashed.set(name, message);
  changed();
}

function activated(name: string) {
  return Date.now() - (live.get(name)?.pressedAt ?? 0) < ACTIVATION_MS;
}

function bumpRender(name: string) {
  renders.set(name, (renders.get(name) ?? 0) + 1);
}

function effect(name: string, method: string, args: unknown[]) {
  const [first, second] = args as [
    Record<string, unknown> | string,
    Record<string, unknown>?,
  ];
  switch (method) {
    case "ui.open": {
      const view = first as {
        id: string;
        title: string;
        site?: ModView["site"];
        icon?: string;
        focus?: boolean;
      };
      const entry: ModView = {
        name,
        id: String(view.id),
        title: String(view.title),
        site: view.site === "Pane" ? "Pane" : "SidebarTab",
        icon: view.icon,
      };
      views = [
        ...views.filter((v) => !(v.name === name && v.id === entry.id)),
        entry,
      ];
      if (view.focus !== false) ctx?.openView(entry);
      break;
    }
    case "ui.close": {
      const id = String((first as { id: string }).id);
      views = views.filter((v) => !(v.name === name && v.id === id));
      break;
    }
    case "ui.toast": {
      const key = `${name}:${nextToast++}`;
      const requested = Number(second?.timeoutMs ?? DEFAULT_TOAST_MS);
      const timeoutMs =
        Number.isFinite(requested) && requested > 0
          ? Math.min(requested, MAX_TOAST_MS)
          : DEFAULT_TOAST_MS;
      const own = toasts.filter((t) => t.name === name);
      const dropped = new Set(
        own.slice(0, own.length - MAX_TOASTS_PER_MOD + 1).map((t) => t.key),
      );
      toasts = [
        ...toasts.filter((t) => !dropped.has(t.key)),
        {
          key,
          name,
          text: String(first),
          tone: second?.tone as Tone,
          timeoutMs,
        },
      ];
      setTimeout(() => {
        toasts = toasts.filter((t) => t.key !== key);
        changed();
      }, timeoutMs);
      break;
    }
    case "ui.status": {
      const [id, text, options = {}] = args as [
        string,
        string | null,
        Record<string, unknown>?,
      ];
      statuses = statuses.filter((s) => !(s.name === name && s.id === id));
      if (text !== null) {
        statuses = [
          ...statuses,
          {
            name,
            id,
            text: String(text),
            tone: options.tone as Tone,
            icon: options.icon as string,
          },
        ];
      }
      break;
    }
    case "ui.invalidate":
      bumpRender(name);
      break;
    // Only right after a press: a mod sees tool output, so these must not leak it unasked.
    case "ui.openUrl":
      if (activated(name) && /^https?:\/\//i.test(String(first)))
        void openUrl(String(first)).catch(() => undefined);
      return;
    case "ui.openFile":
      if (activated(name))
        ctx?.openFile(String(first), second?.line as number | undefined);
      return;
    default:
      // ponytail: command.register is accepted but command.run is not wired to the composer yet.
      return;
  }
  changed();
}

async function onFrameMessage(name: string, message: FrameMessage) {
  const mod = live.get(name);
  if (!mod) return;
  switch (message.kind) {
    case "ready":
      mod.ready = true;
      mod.subscriptions = new Set(message.subscriptions.map((s) => s.name));
      bumpRender(name);
      changed();
      return;
    case "result": {
      const resolve = mod.waits.get(message.id);
      mod.waits.delete(message.id);
      if (!message.ok) console.warn(`[mod ${name}]`, message.error);
      resolve?.(message.ok ? message.value : null);
      return;
    }
    case "call": {
      if (mod.inFlight >= MAX_IN_FLIGHT || !ctx) {
        mod.frame.post({
          kind: "result",
          id: message.id,
          ok: false,
          error: "Too many calls in flight",
        });
        return;
      }
      if (message.method === "ui.copy" && !activated(name)) {
        mod.frame.post({
          kind: "result",
          id: message.id,
          ok: true,
          value: { isCopied: false },
        });
        return;
      }
      mod.inFlight++;
      try {
        const value = await callRpc(
          ctx,
          name,
          message.method as RpcMethod,
          message.args,
        );
        mod.frame.post({
          kind: "result",
          id: message.id,
          ok: true,
          value: value ?? null,
        });
      } catch (error) {
        mod.frame.post({
          kind: "result",
          id: message.id,
          ok: false,
          error: String(error),
        });
      } finally {
        mod.inFlight--;
      }
      return;
    }
    case "effect":
      effect(name, message.method, message.args);
      return;
    case "log":
      console[message.level](`[mod ${name}]`, message.message);
      return;
    case "crash":
      crash(name, message.message);
      return;
  }
}

async function start(record: ModRecord) {
  if (live.has(record.name)) return;
  try {
    const code = await bundleMod(ctx?.project() ?? null, record);
    const frame = await mountFrame(
      record.name,
      (m) => void onFrameMessage(record.name, m),
    );
    const mod: Live = {
      frame,
      subscriptions: new Set(),
      ready: false,
      nextId: 1,
      inFlight: 0,
      waits: new Map(),
      lastTrees: new Map(),
      pressedAt: 0,
    };
    live.set(record.name, mod);
    frame.post({
      kind: "load",
      apiVersion: MOD_API_VERSION,
      code,
      config: resolvedConfig(record),
    });
    // A frame that 404s or a register() that never returns still fires `load`.
    // ponytail: no ping after ready, so a worker that hangs later is not caught.
    setTimeout(() => {
      if (live.get(record.name) === mod && !mod.ready)
        crash(record.name, "The mod did not start within 10 s");
    }, READY_TIMEOUT_MS);
  } catch (error) {
    crash(record.name, String(error));
  }
}

let refreshing: Promise<void> = Promise.resolve();

/**
 * Re-reads the installed mods, then starts the enabled ones and stops the
 * rest. `before` runs inside the queue, after any start still in progress.
 */
export function refreshMods(before?: () => void): Promise<void> {
  refreshing = refreshing
    .then(async () => {
      before?.();
      if (!ctx) return;
      records = await listMods(ctx.project()).catch(() => records);
      const enabled = new Set(
        records
          .filter((r) => r.enabled && !r.error && !crashed.has(r.name))
          .map((r) => r.name),
      );
      for (const name of [...live.keys()]) if (!enabled.has(name)) stop(name);
      views = views.filter((v) => enabled.has(v.name) || crashed.has(v.name));
      changed();
      await Promise.all(records.filter((r) => enabled.has(r.name)).map(start));
    })
    .catch((error) => console.error("[mods]", error));
  return refreshing;
}

/** Called once by App with the window's state accessors. */
export function startModHost(context: ModHostContext) {
  ctx = context;
  void refreshMods();
}

/** Routes derived events to the mods subscribed to them. */
export function emitModEvents(emissions: readonly ModEmission[]) {
  for (const emission of emissions) {
    for (const [name, mod] of live) {
      if (emission.type === "tool.done") {
        if (mod.ready && mod.subscriptions.has("tool.call")) {
          mod.frame.post({
            kind: "tool.done",
            callId: emission.callId,
            result: emission.result,
          });
        }
      } else if (mod.subscriptions.has(emission.name)) {
        void send(name, emission.name, emission.event);
      }
    }
  }
}

export const modHost: ModHost = {
  views: () => views,
  status: () => statuses,
  toasts: () => toasts,
  async render(name, site, requestId, props, sessionId) {
    const mod = live.get(name);
    if (!mod?.ready) return null;
    const request = send(name, "ui.render", {
      component: site,
      requestId,
      sessionId,
      surface: "desktop",
      props,
    }).then((tree) => {
      mod.lastTrees.set(requestId, (tree as Element | null) ?? null);
      return (tree as Element | null) ?? null;
    });
    // A slow render keeps showing the last tree.
    const timeout = new Promise<Element | null>((resolve) =>
      setTimeout(
        () => resolve(mod.lastTrees.get(requestId) ?? null),
        RENDER_TIMEOUT_MS,
      ),
    );
    return Promise.race([request, timeout]);
  },
  invoke(name, cb, args) {
    const mod = live.get(name);
    if (!mod?.ready) return;
    mod.pressedAt = Date.now();
    mod.frame.post({ kind: "callback", cb, args });
  },
  focus(name, site, requestId) {
    void send(name, "ui.focus", { component: site, requestId });
  },
  subscribe(listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  renderVersion: (name) => renders.get(name) ?? 0,
  mods: () =>
    records.map((r) => ({
      manifest: r.manifest ?? ({ name: r.name } as ModManifest),
      enabled: r.enabled,
      source: r.source,
      dir: r.dir,
      shadows: r.shadows,
      config: r.manifest ? resolvedConfig(r) : {},
      error: r.error ?? undefined,
      crashed: crashed.has(r.name) || undefined,
    })),
  async setEnabled(name, enabled) {
    const record = records.find((r) => r.name === name);
    if (!record) return;
    crashed.delete(name);
    // The bundle key is the manifest hash, so pick up source edits on enable.
    if (enabled) forgetBundle(name);
    await setModEnabled(
      ctx?.project() ?? null,
      name,
      enabled,
      record.manifestHash,
    );
    await refreshMods();
  },
  async setConfig(name, values) {
    const record = records.find((r) => r.name === name);
    if (!record) return;
    try {
      localStorage.setItem(CONFIG_KEY + record.dir, JSON.stringify(values));
    } catch {
      // Storage can be unavailable; the values still apply to this load.
    }
    // The bundle is unchanged, so restart without re-bundling.
    await refreshMods(() => stop(name));
  },
  async reload(name) {
    await refreshMods(() => {
      stop(name);
      forgetBundle(name);
      crashed.delete(name);
    });
  },
  async loadUnpacked(path) {
    await addUnpackedMod(path);
    await refreshMods();
  },
};

/** The window's mod host; re-renders the caller whenever the host changes. */
export function useModHost(): ModHost {
  const snapshot = () => version;
  useSyncExternalStore(modHost.subscribe, snapshot, snapshot);
  return modHost;
}

/** Only the open views; for callers that should not re-render on toasts or status. */
export function useModViews(): ModView[] {
  return useSyncExternalStore(modHost.subscribe, modHost.views, modHost.views);
}

/** Bumps when `name` should render again; see `ModHost.renderVersion`. */
export function useModRenderVersion(name: string): number {
  const snapshot = () => modHost.renderVersion(name);
  return useSyncExternalStore(modHost.subscribe, snapshot, snapshot);
}
