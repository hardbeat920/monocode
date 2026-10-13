import {
  acquireHarnessBridge,
  copyMonoCodexThreads,
  killChild,
  prepareMonoCodexStore,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { JsonRpcClient } from "../../core/jsonRpc";
import { isRecoverableThreadResumeError } from "./codexProtocol";

type Thread = { id: string; path?: string | null };
type PreparedContext = { config: Record<string, unknown>; hasThread: boolean };
/** config/read includes unset options at every depth. Codex converts JSON null
 * overrides to empty TOML strings, which breaks typed tables and arrays. */
export function codexConfigOverrides(
  config: Record<string, unknown>,
  home: string,
): Record<string, unknown> {
  const withoutUnset = (value: unknown): unknown => {
    if (Array.isArray(value))
      return value.filter((item) => item !== null).map(withoutUnset);
    if (value !== null && typeof value === "object") {
      return Object.fromEntries(
        Object.entries(value)
          .filter(([, item]) => item !== null)
          .map(([key, item]) => [key, withoutUnset(item)]),
      );
    }
    return value;
  };
  return {
    ...(withoutUnset(config) as Record<string, unknown>),
    sqlite_home: home,
  };
}
const inFlight = new Map<
  string,
  { threadId?: string; migrationOnly?: boolean; task: Promise<PreparedContext> }
>();

/** Read account configuration in its original home, then retain exact native
 * rollouts privately. No inference or transcript summarization is involved. */
export function prepareCodexMonoContext(input: {
  sessionId: string;
  path: string;
  cwd: string;
  providerAccountId?: string;
  threadId?: string;
  migrationOnly?: boolean;
}): Promise<PreparedContext> {
  const key = `${input.providerAccountId ?? "default"}:${input.sessionId}`;
  const pending = inFlight.get(key);
  if (pending) {
    if (
      pending.threadId === input.threadId &&
      pending.migrationOnly === input.migrationOnly
    )
      return pending.task;
    return pending.task.then(() => prepareCodexMonoContext(input));
  }
  const task = prepare(input).finally(() => {
    if (inFlight.get(key)?.task === task) inFlight.delete(key);
  });
  inFlight.set(key, {
    threadId: input.threadId,
    migrationOnly: input.migrationOnly,
    task,
  });
  return task;
}

async function prepare(
  input: Parameters<typeof prepareCodexMonoContext>[0],
): Promise<PreparedContext> {
  const store = await prepareMonoCodexStore(
    input.providerAccountId,
    input.threadId,
  );
  if (input.migrationOnly && store.hasThread)
    return { config: {}, hasThread: true };
  const release = await acquireHarnessBridge();
  const id = `${input.sessionId}:codex-store:${input.providerAccountId ?? "default"}`;
  const rpc: JsonRpcClient = new JsonRpcClient(
    id,
    {
      onRequest: (requestId, method) => {
        if (method === "currentTime/read")
          return rpc.respond(requestId, {
            currentTimeAt: Math.floor(Date.now() / 1000),
          });
        return rpc.respondError(requestId, {
          code: -32601,
          message: `Unsupported storage request: ${method}`,
        });
      },
    },
    { includeJsonrpc: false, label: "codex-store" },
  );
  const request = <T = unknown>(method: string, params: unknown) =>
    rpc.request<T>(method, params, 30_000);
  watchChild(
    id,
    (line) => rpc.pushLine(line),
    () => rpc.close(new Error("Codex storage connection exited")),
  );
  let archivedByUs = false;
  try {
    await spawnChild(
      id,
      input.path,
      ["app-server"],
      input.cwd,
      { provider: "codex", id: input.providerAccountId ?? "default" },
      "codex",
    );
    await request("initialize", {
      clientInfo: { name: "monocode", title: "MonoCode", version: "0.1.0" },
      capabilities: { experimentalApi: true },
    });
    await rpc.notify("initialized", undefined);
    // Codex resolves relative configuration paths against the original home.
    // Passing these resolved values preserves profiles, agents and MCP paths.
    const settings = await request<{ config: Record<string, unknown> }>(
      "config/read",
      { cwd: input.cwd },
    );
    const config = codexConfigOverrides(settings.config, store.home);

    if (input.threadId && !store.hasThread) {
      let root: Thread;
      try {
        root = (
          await request<{ thread: Thread }>("thread/read", {
            threadId: input.threadId,
            includeTurns: false,
          })
        ).thread;
      } catch (error) {
        // Match normal Codex recovery for a thread that was already deleted.
        if (isRecoverableThreadResumeError(error))
          return { config, hasThread: false };
        throw error;
      }
      if (!root.path)
        throw new Error("Codex did not return the saved Mono context");
      const descendants = new Map<string, Thread>();
      for (const archived of [false, true]) {
        let cursor: string | undefined;
        do {
          const page = await request<{
            data: Thread[];
            nextCursor?: string | null;
          }>("thread/list", {
            ancestorThreadId: root.id,
            sourceKinds: [
              "cli",
              "vscode",
              "exec",
              "appServer",
              "subAgent",
              "unknown",
            ],
            modelProviders: [],
            archived,
            limit: 100,
            ...(cursor ? { cursor } : {}),
          });
          for (const child of page.data) descendants.set(child.id, child);
          cursor = page.nextCursor ?? undefined;
        } while (cursor);
      }
      if (
        !root.path.replace(/\\/g, "/").split("/").includes("archived_sessions")
      ) {
        await request("thread/archive", { threadId: root.id });
        archivedByUs = true;
      }
      // Archive closes/flushed writers and moves root + descendants. Read the
      // final paths after that operation, then copy their unmodified files.
      const paths: string[] = [];
      for (const thread of [root, ...descendants.values()]) {
        const retained = await request<{ thread: Thread }>("thread/read", {
          threadId: thread.id,
          includeTurns: false,
        });
        if (!retained.thread.path)
          throw new Error("Codex did not retain a Mono thread file");
        paths.push(retained.thread.path);
      }
      await copyMonoCodexThreads(
        input.providerAccountId,
        root.id,
        paths,
        typeof settings.config.sqlite_home === "string"
          ? settings.config.sqlite_home
          : undefined,
      );
      return { config, hasThread: true };
    }
    return { config, hasThread: store.hasThread };
  } catch (error) {
    if (archivedByUs && input.threadId) {
      // A failed copy leaves the original intact and restores its visibility.
      await request("thread/unarchive", { threadId: input.threadId }).catch(
        () => undefined,
      );
    }
    throw error;
  } finally {
    rpc.close();
    unwatchChild(id);
    await killChild(id).catch(() => undefined);
    release();
  }
}
