import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type LinePayload = { sessionId: string; line: string; pid?: number };
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
// Pids retired by kill/respawn but not yet replaced: stdout frames emitted
// before the kill can arrive during the respawn window while livePid is
// unset, and must not reach the next generation's handlers.
const retiredPids = new Map<string, Set<number>>();
const pendingExit = new Map<
  string,
  Array<{ code: number | null; pid: number }>
>();

function isStaleLine(sessionId: string, pid: number | undefined): boolean {
  if (pid == null || pid <= 0) return false;
  const current = livePid.get(sessionId);
  if (current != null) return pid !== current;
  return retiredPids.get(sessionId)?.has(pid) ?? false;
}

function retirePid(sessionId: string) {
  const pid = livePid.get(sessionId);
  livePid.delete(sessionId);
  if (pid == null || pid <= 0) return;
  const retired = retiredPids.get(sessionId) ?? new Set<number>();
  retired.add(pid);
  if (retired.size > 4) {
    const first = retired.values().next().value;
    if (first != null) retired.delete(first);
  }
  retiredPids.set(sessionId, retired);
}

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
        const { sessionId, line, pid } = event.payload;
        if (isStaleLine(sessionId, pid)) return;
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
        const { sessionId, line, pid } = event.payload;
        if (isStaleLine(sessionId, pid)) return;
        stderrHandlers.get(sessionId)?.(line);
      }),
    ),
    register(
      listen<ExitPayload>("harness-exit", (event) => {
        const { sessionId, code, pid } = event.payload;
        const handler = exitHandlers.get(sessionId);
        if (!handler || pid == null || pid <= 0) return;
        const currentPid = livePid.get(sessionId);
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
  retiredPids.clear();
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
): Promise<number> {
  retirePid(sessionId);
  pendingExit.delete(sessionId);
  const pid = await invoke<number>("harness_spawn", {
    sessionId,
    command,
    args,
    cwd,
  });
  if (typeof pid !== "number" || pid <= 0) return 0;
  livePid.set(sessionId, pid);
  retiredPids.delete(sessionId);
  const exits = pendingExit.get(sessionId);
  pendingExit.delete(sessionId);
  const exited = exits?.find((event) => event.pid === pid);
  if (exited) {
    livePid.delete(sessionId);
    exitHandlers.get(sessionId)?.(exited.code);
  }
  return pid;
}

export function writeChild(
  sessionId: string,
  line: string,
  expectedPid?: number,
): Promise<void> {
  return invoke("harness_write", { sessionId, line, expectedPid });
}

export function killChild(sessionId: string): Promise<void> {
  retirePid(sessionId);
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
  retiredPids.clear();
  pendingExit.clear();
  return invoke("harness_kill_all");
}

export function resolveCursorBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_cursor");
}

export function resolveCodexBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_codex");
}

export function resolveOpenCodeBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_opencode");
}

export function resolveClaudeBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_claude");
}

export function resolvePiBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_pi");
}

export function resolveOmpBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_omp");
}

export function resolveFxBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_fx");
}

export function resolveGrokBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_grok");
}

export function resolveKimiBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_kimi");
}

export function resolveAntigravityBinary(): Promise<{ path: string }> {
  return invoke("harness_resolve_antigravity");
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

export function execChild(
  command: string,
  args: string[],
  cwd?: string,
): Promise<string> {
  return invoke("harness_exec", { command, args, cwd });
}
