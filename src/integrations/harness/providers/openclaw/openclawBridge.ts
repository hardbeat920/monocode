import { AcpClient, type AcpHandlers } from "../../core/acp";
import { recoverAcpSession, unknownAcpRequest } from "../../core/acpLifecycle";
import {
  killChild,
  resolveOpenClawBinary,
  spawnTrustedChild,
  unwatchChild,
  validateOpenClawGatewayWs,
  watchChild,
} from "../../core/child";
import { openClawSessionKey, openClawTransport } from "./openclawTransport";

type OpenClawSetup = { sessionId?: string; session_id?: string };

const SESSION_TIMEOUT_MS = 45_000;
const INIT_TIMEOUT_MS = 12_000;

function sessionIdFromSetup(setup: OpenClawSetup): string | undefined {
  const value = setup.sessionId ?? setup.session_id;
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function isTimeout(error: unknown): boolean {
  return error instanceof Error && error.message.endsWith("timed out");
}

/**
 * Minimal OpenClaw ACP bridge seam. It is deliberately not registered as a
 * HarnessAdapter yet: this validates process/session lifecycle before UI work.
 */
export async function startOpenClawAcpBridge(input: {
  childId: string;
  cwd: string;
  gatewaySessionKey?: string;
  gatewayUrl?: string;
}): Promise<{ acp: AcpClient; sessionId: string; gatewaySessionKey: string; dispose: () => Promise<void> }> {
  const { path } = await resolveOpenClawBinary();
  if (input.gatewayUrl) await validateOpenClawGatewayWs(input.gatewayUrl);
  const descriptor = openClawTransport(path);
  const childId = input.childId;
  let acp!: AcpClient;
  const handlers: AcpHandlers = {
    onRequestRaw: (id, method) =>
      void unknownAcpRequest(acp.respondError.bind(acp), id, method).catch(() => undefined),
  };
  acp = new AcpClient(childId, handlers);
  watchChild(childId, (line) => acp.pushLine(line), () => acp.close(new Error("OpenClaw exited")));
  try {
    await spawnTrustedChild(childId, descriptor, input.cwd);
    await acp.request("initialize", {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
      clientInfo: { name: "monocode", version: "0.1.0" },
      gatewayUrl: input.gatewayUrl,
    }, INIT_TIMEOUT_MS);
    const key = openClawSessionKey(input.gatewaySessionKey);
    const recovery = await recoverAcpSession(undefined, {
      resume: async () => ({ sessionId: key }),
      load: async () => ({ sessionId: key }),
      create: async () => acp.request<OpenClawSetup>("session/new", {
        sessionKey: key,
        gatewayUrl: input.gatewayUrl,
        mcpServers: [],
      }, SESSION_TIMEOUT_MS),
      sessionId: sessionIdFromSetup,
      isTimeout,
    });
  const dispose = async () => {
    acp.close(new Error("OpenClaw bridge disposed"));
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
  };
  return { acp, sessionId: recovery.sessionId, gatewaySessionKey: key, dispose };
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    unwatchChild(childId);
    await killChild(childId).catch(() => undefined);
    throw error;
  }
}

export async function rejectOpenClawRequest(
  acp: AcpClient,
  id: number | string,
  method: string,
): Promise<void> {
  await unknownAcpRequest(acp.respondError.bind(acp), id, method);
}
