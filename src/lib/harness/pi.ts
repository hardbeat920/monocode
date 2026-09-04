import {
  bindSession,
  cancelTurn,
  compactContext,
  forgetSession,
  respondApproval,
  sendTurn,
  setPiBinaryResolver as setFlavorBinaryResolver,
  steerTurn,
  stopSession,
} from "./piFamily";
import { PI_FLAVOR } from "./piFlavor";
import type {
  ApprovalDecision,
  CompactContextInput,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

/**
 * Live Pi adapter. Spawns `pi --mode rpc` with the user's config and extensions
 * loaded (no `--no-extensions`). Todos/subagents packages in `~/.pi/agent`
 * keep working; TUI-only widgets do not appear in MonoCode.
 */
export function sendPiTurn(input: SendTurnInput): Promise<void> {
  return sendTurn(PI_FLAVOR, input);
}

export function compactPiContext(input: CompactContextInput): Promise<void> {
  return compactContext(PI_FLAVOR, input);
}

export function steerPiTurn(input: SteerTurnInput): Promise<void> {
  return steerTurn(PI_FLAVOR, input);
}

export function respondPiApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  respondApproval(PI_FLAVOR, sessionId, requestId, decision);
}

export function cancelPiTurn(sessionId: string): Promise<void> {
  return cancelTurn(PI_FLAVOR, sessionId);
}

export function stopPiSession(sessionId: string): Promise<void> {
  return stopSession(PI_FLAVOR, sessionId);
}

export function forgetPiSession(sessionId: string): Promise<void> {
  return forgetSession(PI_FLAVOR, sessionId);
}

export function bindPiSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  bindSession(PI_FLAVOR, threadId, providerSessionId, cwd);
}

/** Test seam. */
export function setPiBinaryResolver(fn: () => Promise<{ path: string }>): void {
  setFlavorBinaryResolver(PI_FLAVOR, fn);
}
