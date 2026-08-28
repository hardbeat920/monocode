import type { HarnessId } from "../session";
import type { PrContent } from "../gitText";
import type { ApprovalDecision, SendTurnInput, SteerTurnInput } from "./types";

export type TitleInput = {
  sessionId: string;
  cwd: string;
  message: string;
};

/**
 * Lifecycle contract for a live harness adapter.
 * App.tsx dispatches through the registry instead of harness-specific branches.
 */
export type HarnessAdapter = {
  id: HarnessId;
  /** True when this adapter can run live turns. */
  live: boolean;
  /** False when the harness cannot accept a follow-up while a turn is running. Default: same as live. */
  canSteer?: boolean;
  sendTurn(input: SendTurnInput): Promise<void>;
  steerTurn(input: SteerTurnInput): Promise<void>;
  cancelTurn(sessionId: string): Promise<void>;
  respondApproval(
    sessionId: string,
    requestId: number,
    decision: ApprovalDecision,
  ): void;
  /** Kill the child but keep resume state for later rebind. */
  stopSession(sessionId: string): Promise<void>;
  /** Drop resume state and kill the child (delete, harness switch, idle detach). */
  forgetSession(sessionId: string): Promise<void>;
  /** Seed resume state from a restored MonoCode session. */
  bindSession(
    threadId: string,
    providerSessionId: string,
    cwd: string,
  ): void;
  /** Refresh the model catalog overlay when supported. */
  refreshCatalog?(): Promise<void>;
  /** Optional LLM tab title for the first turn. */
  generateTitle?(input: TitleInput): Promise<string | null>;
  /** Optional LLM commit message from staged changes. */
  generateCommitMessage?(cwd: string): Promise<string>;
  /** Optional LLM pull request title/body from branch diff context. */
  generatePrContent?(
    cwd: string,
  ): Promise<(PrContent & { base: string; head: string }) | null>;
  /** Optional LLM branch name from a user message. */
  generateBranchName?(cwd: string, message: string): Promise<string | null>;
  /** Optional warmup for text-generation backends. */
  warmupText?(cwd: string): Promise<void>;
};

const adapters = new Map<HarnessId, HarnessAdapter>();

export function registerHarness(adapter: HarnessAdapter): void {
  adapters.set(adapter.id, adapter);
}

export function getHarness(id: HarnessId): HarnessAdapter | undefined {
  return adapters.get(id);
}

export function requireHarness(id: HarnessId): HarnessAdapter {
  const adapter = adapters.get(id);
  if (!adapter) {
    throw new Error(`No harness adapter registered for "${id}"`);
  }
  return adapter;
}

export function isLiveHarness(id: HarnessId): boolean {
  return adapters.get(id)?.live === true;
}

export function listHarnesses(): HarnessAdapter[] {
  return [...adapters.values()];
}

export async function sendHarnessTurn(input: SendTurnInput & { harness: HarnessId }) {
  const adapter = requireHarness(input.harness);
  if (!adapter.live) {
    throw new Error(`${input.harness} is not connected yet`);
  }
  await adapter.sendTurn(input);
}

export function canSteerHarness(id: HarnessId): boolean {
  const adapter = adapters.get(id);
  if (!adapter?.live) return false;
  return adapter.canSteer !== false;
}

export async function steerHarnessTurn(
  input: SteerTurnInput & { harness: HarnessId },
): Promise<void> {
  const adapter = requireHarness(input.harness);
  if (!adapter.live) {
    throw new Error(`${input.harness} is not connected yet`);
  }
  await adapter.steerTurn(input);
}

export async function cancelHarnessTurn(
  harness: HarnessId,
  sessionId: string,
): Promise<void> {
  const adapter = getHarness(harness);
  if (!adapter?.live) return;
  await adapter.cancelTurn(sessionId);
}

export function respondHarnessApproval(
  harness: HarnessId,
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  getHarness(harness)?.respondApproval(sessionId, requestId, decision);
}

export async function stopHarnessSession(
  harness: HarnessId,
  sessionId: string,
): Promise<void> {
  const adapter = getHarness(harness);
  if (!adapter?.live) return;
  await adapter.stopSession(sessionId);
}

export async function forgetHarnessSession(
  harness: HarnessId,
  sessionId: string,
): Promise<void> {
  const adapter = getHarness(harness);
  if (!adapter) return;
  await adapter.forgetSession(sessionId);
}

export function bindHarnessSession(
  harness: HarnessId,
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  getHarness(harness)?.bindSession(threadId, providerSessionId, cwd);
}

/**
 * A catalog probe spawns the provider's own CLI, so it has to stay on demand:
 * refreshing every adapter at launch is what left `pi` running in the
 * background for people who only use Claude and Codex. Once per run is enough
 * — a catalog only changes when the CLI is upgraded, which restarts us anyway.
 */
const refreshedCatalogs = new Set<HarnessId>();

export async function refreshHarnessCatalog(harness: HarnessId): Promise<void> {
  const adapter = getHarness(harness);
  if (!adapter?.refreshCatalog || refreshedCatalogs.has(harness)) return;
  refreshedCatalogs.add(harness);
  try {
    await adapter.refreshCatalog();
  } catch (error: unknown) {
    // A CLI that was missing or busy this time gets another chance later.
    refreshedCatalogs.delete(harness);
    console.debug(`[monocode] ${harness} catalog`, error);
  }
}

export async function refreshHarnessCatalogs(
  harnesses: Iterable<HarnessId>,
): Promise<void> {
  await Promise.all([...new Set(harnesses)].map(refreshHarnessCatalog));
}

export async function generateHarnessTitle(
  harness: HarnessId,
  input: TitleInput,
): Promise<string | null> {
  const adapter = getHarness(harness);
  if (!adapter?.generateTitle) return null;
  return adapter.generateTitle(input);
}

export async function generateHarnessCommitMessage(
  harness: HarnessId,
  cwd: string,
): Promise<string> {
  const adapter = requireHarness(harness);
  if (!adapter.generateCommitMessage) {
    throw new Error(`${harness} does not support commit message generation`);
  }
  return adapter.generateCommitMessage(cwd);
}

export async function generateHarnessPrContent(
  harness: HarnessId,
  cwd: string,
): Promise<(PrContent & { base: string; head: string }) | null> {
  const adapter = getHarness(harness);
  if (!adapter?.generatePrContent) return null;
  return adapter.generatePrContent(cwd);
}

export async function generateHarnessBranchName(
  harness: HarnessId,
  cwd: string,
  message: string,
): Promise<string | null> {
  const adapter = getHarness(harness);
  if (!adapter?.generateBranchName) return null;
  return adapter.generateBranchName(cwd, message);
}

export async function warmupHarnessText(
  harness: HarnessId,
  cwd: string,
): Promise<void> {
  await getHarness(harness)?.warmupText?.(cwd);
}
