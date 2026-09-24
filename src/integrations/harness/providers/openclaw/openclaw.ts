import { AcpClient, type AcpHandlers } from "../../core/acp";
import { recoverAcpSession } from "../../core/acpLifecycle";
import { killChild, resolveOpenClawBinary, spawnTrustedChild, unwatchChild, validateOpenClawGatewayWs, watchChild } from "../../core/child";
import { openClawSessionKey, openClawTransport } from "./openclawTransport";
import { eventsFromAcpUpdate, permissionOptionId, permissionRequestFromAcp } from "../fx/fxProtocol";
import type { ApprovalDecision, HarnessEvent, SendTurnInput, SteerTurnInput } from "../../core/types";

type Setup = { sessionId?: string; session_id?: string };
type Live = { acp: AcpClient; acpSessionId: string; cwd: string; key: string; onEvent: (e: HarnessEvent) => void; cancelled: boolean; mute: boolean; approvals: Map<number, (d: ApprovalDecision) => void>; turns: Promise<void> };
type Resume = { acpSessionId: string; cwd: string; key: string };
const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelled = new Set<string>();
const INIT_TIMEOUT_MS = 12_000;
const SESSION_TIMEOUT_MS = 45_000;
const CONTROL_TIMEOUT_MS = 15_000;
const PROMPT_TIMEOUT_MS = 30 * 60_000;
const CAPS = { fs: { readTextFile: false, writeTextFile: false }, terminal: false };
const setupId = (s: Setup) => { const id = s.sessionId ?? s.session_id; return typeof id === "string" && id.trim() ? id.trim() : undefined; };
const timeout = (e: unknown) => e instanceof Error && e.message.endsWith("timed out");

export async function sendOpenClawTurn(input: SendTurnInput): Promise<void> {
  let live: Live;
  try { live = await ensureLive(input); } catch (e) { cancelled.delete(input.sessionId); throw e; }
  if (cancelled.delete(input.sessionId)) return;
  live.onEvent = input.onEvent;
  live.turns = live.turns.catch(() => undefined).then(async () => {
    live.cancelled = false; live.mute = false;
    if (!input.text.trim()) return;
    try {
      await live.acp.request("session/prompt", { sessionId: live.acpSessionId, prompt: [{ type: "text", text: input.text.trim() }] }, PROMPT_TIMEOUT_MS);
      if (!live.cancelled) { live.onEvent({ type: "message.completed" }); live.onEvent({ type: "reasoning.completed" }); }
    } catch (e) { if (!live.cancelled) throw e; }
  });
  try { await live.turns; } catch (e) { if (liveByThread.get(input.sessionId) === live) await stopOpenClawSession(input.sessionId); throw e; }
}

export async function steerOpenClawTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId); if (!live) throw new Error("No active OpenClaw session");
  if (input.text.trim()) await live.acp.request("session/prompt", { sessionId: live.acpSessionId, prompt: [{ type: "text", text: input.text.trim() }] }, CONTROL_TIMEOUT_MS);
}
export function respondOpenClawApproval(sessionId: string, requestId: number, decision: ApprovalDecision): void { liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision); }
export async function cancelOpenClawTurn(sessionId: string): Promise<void> { const live = liveByThread.get(sessionId); if (!live) { cancelled.add(sessionId); return; } live.cancelled = true; live.mute = true; resolveApprovals(live, "deny"); await live.acp.notify("session/cancel", { sessionId: live.acpSessionId }).catch(() => undefined); live.acp.rejectPending(new Error("cancelled")); }
export async function stopOpenClawSession(sessionId: string): Promise<void> { cancelled.delete(sessionId); const live = liveByThread.get(sessionId); liveByThread.delete(sessionId); if (live) { live.cancelled = true; live.mute = true; resolveApprovals(live, "deny"); } live?.acp.close(); unwatchChild(sessionId); await killChild(sessionId).catch(() => undefined); }
export async function forgetOpenClawSession(sessionId: string): Promise<void> { resumeByThread.delete(sessionId); await stopOpenClawSession(sessionId); }
export function bindOpenClawSession(threadId: string, providerSessionId: string, cwd: string): void { const parts = providerSessionId.split("|", 2); if (threadId && parts[0]?.trim() && cwd.trim()) resumeByThread.set(threadId, { acpSessionId: parts[0].trim(), cwd, key: parts[1] ?? openClawSessionKey(undefined) }); }

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId); if (existing && existing.cwd === input.cwd) return existing; if (existing) await stopOpenClawSession(input.sessionId);
  const prior = resumeByThread.get(input.sessionId); const key = prior?.cwd === input.cwd ? prior.key : openClawSessionKey(undefined);
  const { path } = await resolveOpenClawBinary(); if (input.modelSettings?.gatewayUrl) await validateOpenClawGatewayWs(input.modelSettings.gatewayUrl);
  const childId = input.sessionId; let acp!: AcpClient; let current: Live | null = null;
  const handlers: AcpHandlers = { onNotification: (method, params) => { if (!current || current.mute) return; if (method === "session/update") for (const e of eventsFromAcpUpdate(params)) current.onEvent(e); }, onRequest: (id, method, params) => { if (current && method === "session/request_permission") void permission(current, id, params); else void acp.respondError(id, { code: -32601, message: `Method not found: ${method}` }).catch(() => undefined); } };
  acp = new AcpClient(childId, handlers); watchChild(childId, line => acp.pushLine(line), code => { acp.close(new Error("OpenClaw exited")); current?.onEvent({ type: "session.ended", code }); });
  try {
    await spawnTrustedChild(childId, openClawTransport(path), input.cwd);
    await acp.request("initialize", { protocolVersion: 1, clientCapabilities: CAPS, clientInfo: { name: "monocode", version: "0.1.0" }, gatewayUrl: input.modelSettings?.gatewayUrl }, INIT_TIMEOUT_MS);
    const recovery = await recoverAcpSession(prior?.cwd === input.cwd ? prior.acpSessionId : undefined, { resume: id => acp.request<Setup>("session/resume", { sessionId: id }, SESSION_TIMEOUT_MS), load: id => acp.request<Setup>("session/load", { sessionId: id, cwd: input.cwd, mcpServers: [] }, SESSION_TIMEOUT_MS), create: () => acp.request<Setup>("session/new", { cwd: input.cwd, sessionKey: key, gatewayUrl: input.modelSettings?.gatewayUrl, mcpServers: [] }, SESSION_TIMEOUT_MS), sessionId: setupId, isTimeout: timeout });
    const live: Live = { acp, acpSessionId: recovery.sessionId, cwd: input.cwd, key, onEvent: input.onEvent, cancelled: false, mute: recovery.restored, approvals: new Map(), turns: Promise.resolve() }; current = live; liveByThread.set(childId, live); resumeByThread.set(childId, { acpSessionId: recovery.sessionId, cwd: input.cwd, key }); live.onEvent({ type: "session.providerBound", providerSessionId: `${recovery.sessionId}|${key}` }); live.onEvent({ type: "session.started" }); return live;
  } catch (e) { acp.close(e instanceof Error ? e : new Error(String(e))); await stopOpenClawSession(childId); throw e; }
}
async function permission(live: Live, id: number, params: unknown): Promise<void> { const request = permissionRequestFromAcp(params); if (request.callId) live.onEvent({ type: "tool.updated", callId: request.callId, title: request.title, kind: request.kind, preview: request.preview }); const decision = await new Promise<ApprovalDecision>(resolve => live.approvals.set(id, resolve)); live.approvals.delete(id); await live.acp.respond(id, { outcome: { outcome: "selected", optionId: permissionOptionId(decision, request.optionIds) } }); live.onEvent({ type: "approval.resolved", requestId: id, decision }); }
function resolveApprovals(live: Live, decision: ApprovalDecision) { for (const resolve of live.approvals.values()) resolve(decision); live.approvals.clear(); }
