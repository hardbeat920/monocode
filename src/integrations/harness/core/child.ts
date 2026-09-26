import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  runtimeProviderBinaryPath,
  type ConfigurableBinaryProvider,
} from "../../../features/providers/model/providerBinaryPaths";

type LinePayload = { sessionId: string; line: string };
type ExitPayload = { sessionId: string; code: number | null; pid?: number };
type SsePayload = { sessionId: string; data: string };
type SseEndPayload = { sessionId: string; error?: string | null };

type LineHandler = (line: string) => void;
type ExitHandler = (code: number | null) => void;
type SseHandler = (data: string) => void;
type SseEndHandler = (error?: string) => void;

const lineHandlers = new Map<string, LineHandler>();
const exitHandlers = new Map<string, ExitHandler>();
const lineBuffer = new Map<string, string[]>();
const stderrHandlers = new Map<string, LineHandler>();
const sseHandlers = new Map<string, SseHandler>();
const sseEndHandlers = new Map<string, SseEndHandler>();
const sseBuffer = new Map<string, string[]>();
const livePid = new Map<string, number>();
const pendingExit = new Map<
  string,
  Array<{ code: number | null; pid: number }>
>();

/** True when this exit belongs to the child we currently have spawned. */
export function isCurrentChildExit(
  expectedPid: number | undefined,
  exitedPid: number | undefined,
): boolean {
  if (expectedPid == null || expectedPid <= 0) return false;
  if (exitedPid == null || exitedPid <= 0) return false;
  return exitedPid === expectedPid;
}

const MAX_BUFFERED = 1000;
let bridge: Promise<UnlistenFn[]> | null = null;
let bridgeAttempt: symbol | null = null;
let users = 0;
let teardownTimer: ReturnType<typeof setTimeout> | undefined;

function pushBounded(
  map: Map<string, string[]>,
  sessionId: string,
  item: string,
) {
  const queued = map.get(sessionId) ?? [];
  queued.push(item);
  if (queued.length > MAX_BUFFERED) {
    queued.splice(0, queued.length - MAX_BUFFERED);
  }
  map.set(sessionId, queued);
}

function ensureBridge() {
  if (bridge) return;
  let failed = false;
  const installed: UnlistenFn[] = [];
  const register = (pending: Promise<UnlistenFn>) =>
    pending.then((unlisten) => {
      if (failed) {
        unlisten();
        return () => undefined;
      }
      installed.push(unlisten);
      return unlisten;
    });
  const attempt = Symbol("bridge-installation");
  bridgeAttempt = attempt;
  const installation = Promise.all([
    register(
      listen<LinePayload>("harness-stdout", (event) => {
        const { sessionId, line } = event.payload;
        const handler = lineHandlers.get(sessionId);
        if (handler) {
          handler(line);
          return;
        }
        pushBounded(lineBuffer, sessionId, line);
      }),
    ),
    register(
      listen<LinePayload>("harness-stderr", (event) => {
        const { sessionId, line } = event.payload;
        const handler = stderrHandlers.get(sessionId);
        if (handler) {
          handler(line);
          return;
        }
        // A child that dies on startup explains itself on stderr and nowhere
        // else; without this the only trace left is the generic
        // "Harness process is not running" from the next write.
        console.debug(`[monocode] stderr ${sessionId}`, line);
      }),
    ),
    register(
      listen<ExitPayload>("harness-exit", (event) => {
        const { sessionId, code, pid } = event.payload;
        const handler = exitHandlers.get(sessionId);
        if (!handler || pid == null || pid <= 0) return;
        const currentPid = livePid.get(sessionId);
        console.debug(`[monocode] exit ${sessionId}`, { pid, code, currentPid });
        if (isCurrentChildExit(currentPid, pid)) {
          livePid.delete(sessionId);
          handler(code);
          return;
        }
        if (currentPid != null) return;
        const exits = pendingExit.get(sessionId) ?? [];
        exits.push({ code, pid });
        if (exits.length > 8) exits.splice(0, exits.length - 8);
        pendingExit.set(sessionId, exits);
      }),
    ),
    register(
      listen<SsePayload>("harness-sse", (event) => {
        const { sessionId, data } = event.payload;
        const handler = sseHandlers.get(sessionId);
        if (handler) {
          handler(data);
          return;
        }
        pushBounded(sseBuffer, sessionId, data);
      }),
    ),
    register(
      listen<SseEndPayload>("harness-sse-end", (event) => {
        const { sessionId, error } = event.payload;
        sseEndHandlers.get(sessionId)?.(error ?? undefined);
      }),
    ),
  ]).catch((error: unknown) => {
    failed = true;
    installed.splice(0).forEach((unlisten) => unlisten());
    if (bridgeAttempt === attempt) {
      bridge = null;
      bridgeAttempt = null;
    }
    throw error;
  });
  void installation.catch(() => undefined);
  bridge = installation;
}

function teardownBridge() {
  const pending = bridge;
  bridge = null;
  bridgeAttempt = null;
  lineHandlers.clear();
  exitHandlers.clear();
  lineBuffer.clear();
  stderrHandlers.clear();
  sseHandlers.clear();
  sseEndHandlers.clear();
  sseBuffer.clear();
  livePid.clear();
  pendingExit.clear();
  void pending
    ?.then((fns) => fns.forEach((fn) => fn()))
    .catch(() => undefined);
}

export function startHarnessBridge(): () => void {
  users += 1;
  if (teardownTimer) {
    clearTimeout(teardownTimer);
    teardownTimer = undefined;
  }
  ensureBridge();
  return () => {
    users -= 1;
    if (users > 0) return;
    users = 0;
    teardownTimer = setTimeout(() => {
      teardownTimer = undefined;
      if (users === 0) teardownBridge();
    }, 0);
  };
}

export async function acquireHarnessBridge(): Promise<() => void> {
  const release = startHarnessBridge();
  const installation = bridge;
  try {
    await installation;
    return release;
  } catch (error) {
    release();
    throw error;
  }
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    users = 0;
    if (teardownTimer) {
      clearTimeout(teardownTimer);
      teardownTimer = undefined;
    }
    teardownBridge();
  });
}

export function watchChild(
  sessionId: string,
  onLine: LineHandler,
  onExit: ExitHandler,
  onStderr?: LineHandler,
) {
  const queued = lineBuffer.get(sessionId);
  lineBuffer.delete(sessionId);
  lineHandlers.set(sessionId, onLine);
  exitHandlers.set(sessionId, onExit);
  if (onStderr) stderrHandlers.set(sessionId, onStderr);
  if (queued) queued.forEach(onLine);
}

export function unwatchChild(sessionId: string) {
  lineHandlers.delete(sessionId);
  exitHandlers.delete(sessionId);
  lineBuffer.delete(sessionId);
  stderrHandlers.delete(sessionId);
  pendingExit.delete(sessionId);
}

export function watchSse(
  sessionId: string,
  onData: SseHandler,
  onEnd?: SseEndHandler,
) {
  const queued = sseBuffer.get(sessionId);
  sseBuffer.delete(sessionId);
  sseHandlers.set(sessionId, onData);
  if (onEnd) sseEndHandlers.set(sessionId, onEnd);
  if (queued) queued.forEach(onData);
}

export function unwatchSse(sessionId: string) {
  sseHandlers.delete(sessionId);
  sseEndHandlers.delete(sessionId);
  sseBuffer.delete(sessionId);
}

export async function spawnChild(
  sessionId: string,
  command: string,
  args: string[],
  cwd: string,
  account?: { provider: "claude" | "codex"; id: string },
  binaryProvider?: ConfigurableBinaryProvider,
): Promise<void> {
  livePid.delete(sessionId);
  pendingExit.delete(sessionId);
  const binaryPath = binaryProvider
    ? runtimeProviderBinaryPath(binaryProvider)
    : undefined;
  const pid = await invoke<number>("harness_spawn", {
    sessionId,
    command,
    args,
    cwd,
    account,
    binaryProvider,
    binaryPath,
  });
  console.debug(`[monocode] spawn ${sessionId}`, { command, pid, cwd });
  if (typeof pid !== "number" || pid <= 0) return;
  livePid.set(sessionId, pid);
  const exits = pendingExit.get(sessionId);
  pendingExit.delete(sessionId);
  const exited = exits?.find((event) => event.pid === pid);
  if (!exited) return;
  livePid.delete(sessionId);
  exitHandlers.get(sessionId)?.(exited.code);
}

export function writeChild(sessionId: string, line: string): Promise<void> {
  return invoke<void>("harness_write", { sessionId, line }).catch(
    (error: unknown) => {
      // The write is the first thing that notices a child which never started
      // or already died, so name the session it was meant for.
      console.debug(`[monocode] write failed ${sessionId}`, error);
      throw error;
    },
  );
}

export function killChild(sessionId: string): Promise<void> {
  livePid.delete(sessionId);
  pendingExit.delete(sessionId);
  unwatchChild(sessionId);
  return invoke("harness_kill", { sessionId });
}

export function killAllChildren(): Promise<void> {
  lineHandlers.clear();
  exitHandlers.clear();
  lineBuffer.clear();
  stderrHandlers.clear();
  sseHandlers.clear();
  sseEndHandlers.clear();
  sseBuffer.clear();
  livePid.clear();
  pendingExit.clear();
  return invoke("harness_kill_all");
}

type ResolvedHarnessBinary = { path: string; args?: string[] };

async function resolveHarnessBinary(
  provider: ConfigurableBinaryProvider,
  binaryPath?: string | null,
): Promise<ResolvedHarnessBinary> {
  const configuredPath =
    binaryPath === undefined
      ? runtimeProviderBinaryPath(provider)
      : binaryPath?.trim();
  if (configuredPath) {
    return invoke("harness_resolve_configured", {
      provider,
      binaryPath: configuredPath,
    });
  }
  const command: Record<ConfigurableBinaryProvider, string> = {
    claude: "harness_resolve_claude",
    codex: "harness_resolve_codex",
    cursor: "harness_resolve_cursor",
    grok: "harness_resolve_grok",
    opencode: "harness_resolve_opencode",
    pi: "harness_resolve_pi",
    omp: "harness_resolve_omp",
    fx: "harness_resolve_fx",
    hermes: "harness_resolve_hermes",
    antigravity: "harness_resolve_antigravity",
  };
  return invoke(command[provider]);
}

export function resolveCursorBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("cursor", binaryPath);
}

export function resolveCodexBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("codex", binaryPath);
}

export function resolveOpenCodeBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("opencode", binaryPath);
}

export function resolveClaudeBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("claude", binaryPath);
}

export function resolvePiBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("pi", binaryPath);
}

export function resolveOmpBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("omp", binaryPath);
}

export function resolveFxBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("fx", binaryPath);
}

export function resolveGrokBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("grok", binaryPath);
}

export function resolveHermesBinary(
  binaryPath?: string | null,
): Promise<{ path: string }> {
  return resolveHarnessBinary("hermes", binaryPath);
}

export function resolveAntigravityBinary(
  binaryPath?: string | null,
): Promise<{ path: string; args: string[] }> {
  return resolveHarnessBinary("antigravity", binaryPath) as Promise<{
    path: string;
    args: string[];
  }>;
}

export function freeHarnessPort(): Promise<number> {
  return invoke("harness_free_port");
}

export function harnessHttp(input: {
  url: string;
  method: string;
  headers?: Record<string, string>;
  body?: string;
  timeoutMs?: number;
}): Promise<{ status: number; body: string }> {
  return invoke("harness_http", input);
}

export function openHarnessSse(
  sessionId: string,
  url: string,
  headers?: Record<string, string>,
): Promise<void> {
  return invoke("harness_sse_open", { sessionId, url, headers });
}

export function closeHarnessSse(sessionId: string): Promise<void> {
  unwatchSse(sessionId);
  return invoke("harness_sse_close", { sessionId });
}

export type HarnessBinaryInspection = {
  path: string;
  version?: string;
  error?: string;
};

export function inspectHarnessBinary(
  provider: ConfigurableBinaryProvider,
  binaryPath?: string | null,
): Promise<HarnessBinaryInspection> {
  return resolveHarnessBinary(provider, binaryPath).then(async (resolved) => {
    if (provider === "antigravity") {
      return { path: resolved.path, version: "ACP server" };
    }
    try {
      const version = (
        await execChild(
          resolved.path,
          ["--version"],
          undefined,
          provider,
          binaryPath,
        )
      ).trim();
      return /\d+\.\d+\.\d+/.test(version)
        ? { path: resolved.path, version }
        : { path: resolved.path, error: "CLI returned no valid version." };
    } catch (error) {
      return {
        path: resolved.path,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  });
}

export function execChild(
  command: string,
  args: string[],
  cwd?: string,
  binaryProvider?: ConfigurableBinaryProvider,
  binaryPathOverride?: string | null,
): Promise<string> {
  const binaryPath =
    binaryPathOverride === undefined && binaryProvider
      ? runtimeProviderBinaryPath(binaryProvider)
      : binaryPathOverride;
  return invoke("harness_exec", {
    command,
    args,
    cwd,
    binaryProvider,
    binaryPath,
  });
}
