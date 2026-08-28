import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  bridge = Promise.all([
    listen<LinePayload>("harness-stdout", (event) => {
      const { sessionId, line } = event.payload;
      const handler = lineHandlers.get(sessionId);
      if (handler) {
        handler(line);
        return;
      }
      pushBounded(lineBuffer, sessionId, line);
    }),
    listen<LinePayload>("harness-stderr", (event) => {
      const { sessionId, line } = event.payload;
      stderrHandlers.get(sessionId)?.(line);
    }),
    listen<ExitPayload>("harness-exit", (event) => {
      const { sessionId, code, pid } = event.payload;
      if (!isCurrentChildExit(livePid.get(sessionId), pid)) return;
      livePid.delete(sessionId);
      exitHandlers.get(sessionId)?.(code);
    }),
    listen<SsePayload>("harness-sse", (event) => {
      const { sessionId, data } = event.payload;
      const handler = sseHandlers.get(sessionId);
      if (handler) {
        handler(data);
        return;
      }
      pushBounded(sseBuffer, sessionId, data);
    }),
    listen<SseEndPayload>("harness-sse-end", (event) => {
      const { sessionId, error } = event.payload;
      sseEndHandlers.get(sessionId)?.(error ?? undefined);
    }),
  ]);
}

function teardownBridge() {
  const pending = bridge;
  bridge = null;
  lineHandlers.clear();
  exitHandlers.clear();
  lineBuffer.clear();
  stderrHandlers.clear();
  sseHandlers.clear();
  sseEndHandlers.clear();
  sseBuffer.clear();
  livePid.clear();
  void pending?.then((fns) => fns.forEach((fn) => fn()));
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

/**
 * `ttlMs` hands the kill to Rust for one-shot children. Our own cleanup is a
 * `finally` in the webview, which never runs if the window reloads or closes
 * mid-probe — that stranded a `pi` catalog probe at 1 GB for a whole session.
 * Pass it only for children we always expect to kill ourselves.
 */
export async function spawnChild(
  sessionId: string,
  command: string,
  args: string[],
  cwd: string,
  ttlMs?: number,
): Promise<void> {
  const pid = await invoke<number>("harness_spawn", {
    sessionId,
    command,
    args,
    cwd,
    ttlMs,
  });
  if (typeof pid === "number" && pid > 0) livePid.set(sessionId, pid);
}

export function writeChild(sessionId: string, line: string): Promise<void> {
  return invoke("harness_write", { sessionId, line });
}

export function killChild(sessionId: string): Promise<void> {
  livePid.delete(sessionId);
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
