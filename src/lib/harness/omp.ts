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
import { OMP_FLAVOR } from "./piFlavor";
import type {
  ApprovalDecision,
  CompactContextInput,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

/**
 * Live omp (oh-my-pi) adapter. Spawns `omp --mode rpc` with the user's config
 * and extensions loaded (no `--no-extensions`), so plugins in `~/.omp/agent`
 * keep working; TUI-only widgets do not appear in MonoCode. omp is a fork of
 * Pi and speaks the same RPC protocol, so both run on the `piFamily` core.
 */
export function sendOmpTurn(input: SendTurnInput): Promise<void> {
  return sendTurn(OMP_FLAVOR, input);
}

export function compactOmpContext(input: CompactContextInput): Promise<void> {
  return compactContext(OMP_FLAVOR, input);
}

export function steerOmpTurn(input: SteerTurnInput): Promise<void> {
  return steerTurn(OMP_FLAVOR, input);
}

export function respondOmpApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  respondApproval(OMP_FLAVOR, sessionId, requestId, decision);
}

export function cancelOmpTurn(sessionId: string): Promise<void> {
  return cancelTurn(OMP_FLAVOR, sessionId);
}

export function stopOmpSession(sessionId: string): Promise<void> {
  return stopSession(OMP_FLAVOR, sessionId);
}

export function forgetOmpSession(sessionId: string): Promise<void> {
  return forgetSession(OMP_FLAVOR, sessionId);
}

export function bindOmpSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  bindSession(OMP_FLAVOR, threadId, providerSessionId, cwd);
}

/** Test seam. */
export function setOmpBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  setFlavorBinaryResolver(OMP_FLAVOR, fn);
}
