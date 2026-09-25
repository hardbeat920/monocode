import type { JsonRpcId } from "./jsonRpc";

export type AcpSessionRecovery<T> = {
  sessionId: string;
  setup: T;
  restored: boolean;
};

export type AcpSessionLifecycleHooks<T> = {
  resume: (sessionId: string) => Promise<T>;
  load: (sessionId: string) => Promise<T>;
  create: () => Promise<T>;
  sessionId: (setup: T) => string | undefined;
  isTimeout?: (error: unknown) => boolean;
};

/**
 * Provider-neutral ACP recovery ladder. Providers own wire parameters and
 * decoding; this helper owns the resume/load/new ordering and timeout rule.
 */
export async function recoverAcpSession<T>(
  previousSessionId: string | undefined,
  hooks: AcpSessionLifecycleHooks<T>,
): Promise<AcpSessionRecovery<T>> {
  if (previousSessionId) {
    try {
      const setup = await hooks.resume(previousSessionId);
      const sessionId = hooks.sessionId(setup) ?? previousSessionId;
      return { sessionId, setup, restored: true };
    } catch (resumeError) {
      if (hooks.isTimeout?.(resumeError)) throw resumeError;
      try {
        const setup = await hooks.load(previousSessionId);
        const sessionId = hooks.sessionId(setup) ?? previousSessionId;
        return { sessionId, setup, restored: true };
      } catch (loadError) {
        if (hooks.isTimeout?.(loadError)) throw loadError;
      }
    }
  }

  const setup = await hooks.create();
  const sessionId = hooks.sessionId(setup);
  if (!sessionId) throw new Error("ACP session setup did not return a session id");
  return { sessionId, setup, restored: false };
}

export type AcpRawRequestRouter = {
  onRequestRaw: (
    id: JsonRpcId,
    method: string,
    params: unknown,
  ) => void | Promise<void>;
};

/** Unknown ACP methods must fail closed; never turn protocol input into a command. */
export function unknownAcpRequest(
  respondError: (id: JsonRpcId, error: { code: number; message: string }) => Promise<void>,
  id: JsonRpcId,
  method: string,
): Promise<void> {
  return respondError(id, {
    code: -32601,
    message: `Method not found: ${method}`,
  });
}
