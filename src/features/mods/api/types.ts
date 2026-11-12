/**
 * MonoMods public API, version 1.
 *
 * This file is the contract between Monocode and third-party mods. It must
 * stay free of imports from the app so mods can vendor it as `monocode.d.ts`.
 * Names follow Claude Code Mods where a concept exists there, so ports stay
 * mechanical. See docs/monomods.md for the design.
 */

export const MOD_API_VERSION = 1;

// ---------------------------------------------------------------------------
// Manifest (mod.json)
// ---------------------------------------------------------------------------

/** Binaries a mod may spawn. "sh"/"bash"/"rm" are shown as full shell access. */
export type ProcessPermission = string;

/**
 * Glob over absolute paths. `$PROJECT` (the active project root) and `$HOME`
 * are expanded by the host, e.g. "$PROJECT/**", "$HOME/.gitconfig".
 */
export type FsGlob = string;

export type ModPermissions = {
  process?: ProcessPermission[];
  fs?: { read?: FsGlob[] };
  /** Hostnames for `$.http`; loopback and private ranges are always refused. */
  http?: string[];
  /** Environment variable names `$.env.get` may read. */
  env?: string[];
};

export type UserConfigField =
  | {
      type: "string";
      title: string;
      description?: string;
      default?: string;
      /** Renders as a select when present. */
      options?: string[];
    }
  | { type: "boolean"; title: string; description?: string; default?: boolean }
  | { type: "number"; title: string; description?: string; default?: number };

export type ModManifest = {
  /** Folder-safe id, unique per install: /^[a-z0-9][a-z0-9-]{0,63}$/. */
  name: string;
  /** Semver of the mod itself. */
  version: string;
  description?: string;
  /** MonoMods API the mod was written against. */
  apiVersion: typeof MOD_API_VERSION;
  /** Entry module, relative to the mod folder, e.g. "src/register.tsx". */
  entry: string;
  permissions?: ModPermissions;
  userConfig?: Record<string, UserConfigField>;
};

export type UserConfigValue = string | number | boolean;
export type UserConfigValues = Record<string, UserConfigValue>;

// ---------------------------------------------------------------------------
// Entry point and handlers
// ---------------------------------------------------------------------------

/** `export const register: Register = (on, $) => { ... }` */
export type Register = (on: On, $: Engine) => void | Promise<void>;

/**
 * Handlers run in registration order. Calling `next` passes the event on; a
 * handler that returns without calling it ends the chain. Every event in v1
 * is observe-only except where its result type says otherwise.
 */
export type Handler<N extends ModEventName> = (
  $: Engine,
  event: ModEvents[N]["event"],
  next: (event?: ModEvents[N]["event"]) => Promise<ModEvents[N]["result"]>,
) => ModEvents[N]["result"] | Promise<ModEvents[N]["result"]>;

export type On = <N extends ModEventName>(
  name: N,
  filter: ModEvents[N]["filter"] | null,
  handler: Handler<N>,
) => void;

// ---------------------------------------------------------------------------
// Events (derived from the provider-neutral HarnessEvent stream)
// ---------------------------------------------------------------------------

/** Claude tool names; other providers are normalised onto these by tool kind. */
export type ToolName =
  | "Bash"
  | "Read"
  | "Edit"
  | "Write"
  | "NotebookEdit"
  | "Grep"
  | "Glob"
  | "Agent"
  | (string & {});

export type ToolCallEvent = {
  sessionId: string;
  callId: string;
  tool: ToolName;
  /** Subagent that made the call, when it is not the main agent. */
  agentId?: string;
  /** Shell command, for Bash. */
  command?: string;
  /** Primary path for file tools. */
  file_path?: string;
  /** Every path the call touched. */
  paths?: string[];
};

export type ToolCallResult = {
  isError: boolean;
  /** The user or a policy refused the call. */
  deny?: boolean;
  result?: { stdout?: string };
};

export type SessionInfo = {
  id: string;
  /** Worktree-aware working directory. */
  cwd: string;
  /** Project root as the sidebar shows it. */
  projectCwd: string;
  provider: string;
  isInteractive: boolean;
};

export type RenderProps = {
  title: string;
  isFocused: boolean;
  /** Coarse width hint in character columns; layout should use CSS-like flex. */
  bodyColumns: number;
};

export type ModEvents = {
  "session.start": {
    filter: { isInteractive?: boolean };
    event: SessionInfo;
    result: void;
  };
  "session.end": { filter: {}; event: { sessionId: string }; result: void };
  /** Turn started or finished (busy flipped). */
  "turn.start": { filter: {}; event: { sessionId: string }; result: void };
  "turn.complete": {
    filter: {};
    event: {
      sessionId: string;
      /** Wall time of the turn; the host stamps start and end. */
      durationMs: number;
      /** From the harness `turn.metrics` event; undefined when the provider sends none. */
      metrics?: {
        inputTokens?: number;
        outputTokens?: number;
        cacheReadTokens?: number;
        cacheWriteTokens?: number;
      };
    };
    result: void;
  };
  /** Context-window level after the harness's latest request (harness `context` event). */
  "context.update": {
    filter: {};
    event: { sessionId: string; used?: number; window?: number };
    result: void;
  };
  /**
   * Fires when the call starts; `await next(e)` resolves when it reaches a
   * terminal status. Observe-only: the event cannot be rewritten or denied.
   */
  "tool.call": {
    filter: { tool?: ToolName };
    event: ToolCallEvent;
    result: ToolCallResult;
  };
  "command.run": {
    filter: { command: string };
    event: { sessionId?: string; command: string; args: string };
    result: { text?: string } | void;
  };
  /** Active project or session changed in the window. */
  "workspace.change": {
    filter: {};
    event: { projectCwd: string | null; sessionId: string | null };
    result: void;
  };
  "ui.render": {
    filter: { component: RenderSite; requestId?: string };
    event: {
      component: RenderSite;
      requestId: string;
      sessionId?: string;
      surface: "desktop";
      props: RenderProps;
    };
    result: Element | null;
  };
  "ui.focus": {
    filter: { component: RenderSite; requestId?: string };
    event: { component: RenderSite; requestId: string };
    result: void;
  };
  /** later: "prompt.submit" (context injection), "ui.message". */
};

export type ModEventName = keyof ModEvents;

// ---------------------------------------------------------------------------
// The $ engine
// ---------------------------------------------------------------------------

export type ProcessResult = {
  exitCode: number;
  stdout: string;
  stderr: string;
  isStdoutTruncated: boolean;
};

export type FsEntry = {
  name: string;
  kind: "file" | "dir" | "other";
  mtimeMs: number;
  isLink: boolean;
};

export type HttpResponse = {
  status: number;
  headers: Record<string, string>;
  body: string;
};

export type Timer = { cancel(): void };

export type Json =
  | null
  | boolean
  | number
  | string
  | Json[]
  | { [key: string]: Json };

/** Per-mod state slot. Reads inside `ui.render` subscribe; writes re-render. */
export type Atom<T> = { key: string; initial: T };

export type Engine = {
  /** Spawned with the login-shell PATH; throws when the binary is not found. */
  process: {
    run(
      argv: string[],
      options?: { cwd?: string; timeoutMs?: number; env?: Record<string, string> },
    ): Promise<ProcessResult>;
  };
  fs: {
    read(path: string): Promise<string>;
    list(dir: string): Promise<FsEntry[]>;
    /** Throws when the path does not exist. */
    stat(path: string): Promise<{ kind: FsEntry["kind"]; mtimeMs: number }>;
  };
  http: {
    request(
      url: string,
      options?: {
        method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
        headers?: Record<string, string>;
        body?: string;
      },
    ): Promise<HttpResponse>;
  };
  /** Persisted per mod, shared across sessions and windows. */
  store: {
    get<T extends Json>(key: string): Promise<T | undefined>;
    set(key: string, value: Json): Promise<void>;
    delete(key: string): Promise<void>;
  };
  /** In-memory per mod, lost on reload. Sync because it lives in the worker. */
  state: {
    atom<T>(key: string, initial: T & Json): Atom<T>;
    read<T>(atom: Atom<T>): T;
    update<T>(atom: Atom<T>, fn: (value: T) => T): void;
  };
  clock: {
    now(): number;
    every(ms: number, fn: () => void): Timer;
    after(ms: number, fn: () => void): Timer;
  };
  /** Resolved userConfig: manifest defaults overlaid with the user's values. */
  config: UserConfigValues;
  /** Only names listed in `permissions.env`; undefined otherwise. */
  env: { get(name: string): Promise<string | undefined> };
  session: {
    /** The session the current event belongs to, else the active one. */
    current(): Promise<SessionInfo | null>;
    agents(sessionId: string): Promise<{ id: string; name: string; type?: string }[]>;
    /** Earlier tool calls, for replay on load. */
    toolCalls(sessionId: string): Promise<ToolCallEvent[]>;
  };
  workspace: {
    projectCwd(): Promise<string | null>;
    isLightScheme(): Promise<boolean>;
  };
  command: { register(command: { name: string; description: string }): void };
  ui: {
    /** Opens (or focuses) a Pane or SidebarTab; `id` becomes `requestId`. */
    open(view: {
      id: string;
      title: string;
      site?: "Pane" | "SidebarTab";
      icon?: IconName;
      focus?: boolean;
    }): void;
    close(view: { id: string }): void;
    toast(text: string, options?: { tone?: Tone; timeoutMs?: number }): void;
    /** Sets or clears (text = null) this mod's chip in the status footer. */
    status(id: string, text: string | null, options?: { tone?: Tone; icon?: IconName }): void;
    /** Asks every render site of this mod to render again. */
    invalidate(event?: "ui.render"): void;
    copy(text: string): Promise<{ isCopied: boolean }>;
    /** http(s) only. */
    openUrl(url: string): void;
    /** Opens in Monocode's editor. */
    openFile(path: string, options?: { line?: number }): void;
  };
};

// ---------------------------------------------------------------------------
// Element tree
// ---------------------------------------------------------------------------

/**
 * Semantic colours. The host maps them onto theme tokens, so mods follow
 * light/dark and user themes. `read`/`write`/`git` are the activity tones.
 */
export type Tone =
  | "content"
  | "muted"
  | "subtle"
  | "accent"
  | "success"
  | "danger"
  | "warning"
  | "info"
  | "read"
  | "write"
  | "git"
  | "purple"
  | "orange"
  | "green"
  | "teal"
  | "pink"
  | "cyan"
  | "yellow"
  | "blue"
  | "red";

/** A Hugeicons name from Monocode's icon map, e.g. "branch", "refresh". */
export type IconName = string;

/** Callbacks cross the worker boundary as ids; the host sends them back. */
export type Callback<A extends unknown[] = []> = (...args: A) => void;

type Space = 0 | 1 | 2 | 3 | 4 | 6 | 8;

export type BoxProps = {
  key?: string;
  flexDirection?: "row" | "column";
  flexGrow?: number;
  flexShrink?: number;
  flexWrap?: "wrap" | "nowrap";
  alignItems?: "start" | "center" | "end" | "stretch";
  justifyContent?: "start" | "center" | "end" | "between";
  gap?: Space;
  padding?: Space;
  paddingX?: Space;
  paddingY?: Space;
  marginTop?: Space;
  border?: boolean;
  borderColor?: Tone;
  hoverBorderColor?: Tone;
  backgroundColor?: Tone;
  /** Scrolls when content overflows. */
  scroll?: boolean;
  onPress?: Callback;
};

export type TextProps = {
  key?: string;
  color?: Tone;
  bold?: boolean;
  italic?: boolean;
  dimColor?: boolean;
  strikethrough?: boolean;
  mono?: boolean;
  wrap?: "wrap" | "truncate-end" | "truncate-start";
  /** Animated sweep for live items. */
  shimmer?: Tone;
};

export type ButtonProps = {
  key?: string;
  label?: string;
  icon?: IconName;
  /** Tooltip, and the accessible name for icon-only buttons. */
  title?: string;
  plain?: boolean;
  tone?: "default" | "danger";
  disabled?: boolean;
  hotkey?: string;
  onPress: Callback;
};

export type InputProps = {
  key?: string;
  label?: string;
  placeholder?: string;
  value?: string;
  autoFocus?: boolean;
  onInput?: Callback<[value: string]>;
  onSubmit?: Callback<[value: string]>;
};

/** One right-aligned segment of a tree row. */
export type Seg = { text: string; color?: Tone; dimColor?: boolean };

export type TreeRow = {
  id: string;
  depth: number;
  kind: "dir" | "file" | "item";
  label: string;
  open?: boolean;
  /** Absolute path; the host adds the file-type icon and git colour. */
  path?: string;
  icon?: IconName;
  tone?: Tone;
  /** Shimmers and is scrolled into view. */
  live?: Tone;
  right?: Seg[];
};

export type TreeProps = {
  key?: string;
  rows: TreeRow[];
  active?: string;
  /** Scrolled into view once per change. */
  reveal?: string;
  onPress?: Callback<[id: string, mods: { ctrl: boolean; shift: boolean }]>;
  onDoublePress?: Callback<[id: string]>;
  onContextMenu?: Callback<[id: string, mods: { shift: boolean }]>;
  /** Keys the tree does not handle itself (it owns arrows, enter, space). */
  onKey?: Callback<[key: string, mods: { ctrl: boolean; shift: boolean }]>;
};

export type Element =
  | { type: "Box"; props: BoxProps; children: Node[] }
  | { type: "Text"; props: TextProps; children: Node[] }
  | { type: "Button"; props: ButtonProps }
  | { type: "Input"; props: InputProps }
  | { type: "Badge"; props: { key?: string; label: string; tone?: Tone } }
  | { type: "Dot"; props: { key?: string; tone: Tone } }
  | { type: "Icon"; props: { key?: string; name: IconName; tone?: Tone } }
  | { type: "Spinner"; props: { key?: string } }
  | { type: "Skeleton"; props: { key?: string; rows: number } }
  /** CommonMark only; raw HTML is dropped, links must be http(s). */
  | { type: "Markdown"; props: { key?: string; text: string } }
  | { type: "Tree"; props: TreeProps };

export type Node = Element | string | number | false | null | undefined;

/** Where a mod can appear. v1 ships SidebarTab, Pane, AbovePrompt, Status and Toast. */
export type RenderSite =
  | "SidebarTab"
  | "Pane"
  | "Status"
  | "Toast"
  /** A slim band above the composer of the active session. */
  | "AbovePrompt"
  /** later */
  | "ToolUse";

// ---------------------------------------------------------------------------
// Worker <-> host RPC (postMessage via the sandboxed monomod:// frame)
// ---------------------------------------------------------------------------

/**
 * Element trees cross the boundary with callbacks replaced by
 * `{ $cb: id }`; the host answers presses with `callback`.
 */
export type WireCallback = { $cb: string };

export type RpcMethod =
  | "process.run"
  | "fs.read"
  | "fs.list"
  | "fs.stat"
  | "http.request"
  | "store.get"
  | "store.set"
  | "store.delete"
  | "env.get"
  | "session.current"
  | "session.agents"
  | "session.toolCalls"
  | "workspace.projectCwd"
  | "workspace.isLightScheme"
  | "ui.copy";

export type WorkerToHost =
  /** register() finished; the host may start sending events. */
  | { kind: "ready"; subscriptions: { name: ModEventName; filter: unknown }[] }
  | { kind: "call"; id: number; method: RpcMethod; args: unknown[] }
  /** Fire-and-forget effects: ui.open/close/toast/status/invalidate/openUrl/openFile, command.register. */
  | { kind: "effect"; method: string; args: unknown[] }
  | { kind: "result"; id: number; ok: true; value: unknown }
  | { kind: "result"; id: number; ok: false; error: string }
  | { kind: "log"; level: "info" | "warn" | "error"; message: string };

export type HostToWorker =
  | {
      kind: "load";
      apiVersion: typeof MOD_API_VERSION;
      code: string;
      config: UserConfigValues;
    }
  | { kind: "result"; id: number; ok: true; value: unknown }
  | { kind: "result"; id: number; ok: false; error: string }
  /** Dispatch an event through the mod's handler chain; answered by `result`. */
  | { kind: "event"; id: number; name: ModEventName; event: unknown }
  /** A `tool.call` reached a terminal status; resolves the pending `next`. */
  | { kind: "tool.done"; callId: string; result: ToolCallResult }
  | { kind: "callback"; cb: string; args: unknown[] }
  | { kind: "unload" };
