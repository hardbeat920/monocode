import { createHash, randomUUID } from "node:crypto";
import { realpath, stat } from "node:fs/promises";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, isAbsolute, join } from "node:path";
import { renameHostWorktreeBranch, resolveHostWorktree } from "./git-worktrees";
import {
  applyHarnessEvent,
  stopStreaming,
} from "../src/integrations/harness/core/apply";
import { resolveModel } from "../src/features/sessions/model/models";
import { isVisionImage } from "../src/features/sessions/model/attachments";
import type {
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
} from "../src/integrations/harness/core/types";
import {
  acceptProviderDelivery,
  beginProviderDelivery,
  canResumeProviderBinding,
  confirmProviderDeliveryInspection,
  failProviderDelivery,
  markProviderContextDelivered,
  markProviderRequestSubmitted,
  providerBinding,
  recordProviderBound,
  recordProviderContextUsage,
  rememberProviderBinding,
  recoverSubmittedProviderDelivery,
  requiresFreshProviderBinding,
  settleProviderBinding,
  updateProviderHandoff,
  type ProviderBinding,
} from "../src/features/sessions/model/providerContext";
import {
  buildPortableContext,
  buildPortableContextSnapshot,
  currentAttachmentTokens,
  historicalContextAttachments,
  type PortableContext,
} from "../src/features/sessions/model/portableContext";
import type { ContextAssetSnapshot } from "../src/features/sessions/model/contextAssets";
import { snapshotHostContextAssets } from "./context-assets";
import { planComposerSwitch } from "../src/features/sessions/model/handoff";
import { prepareContextTransferInput } from "../src/features/sessions/model/contextTransfer";
import {
  HARNESS_LABEL,
  RUNTIME_MODES,
  canReplaceSessionTitle,
  formatSessionTitle,
  titleFromPrompt,
  type Session,
} from "../src/features/sessions/model/session";
import { namedWorktreeBranch } from "../src/features/source-control/model/worktrees";
import {
  isRemoteProvider,
  type HostCommand,
  type HostSession,
  type CommandReceipt,
  type RemoteProvider,
} from "../src/features/connections/model/protocol";
import type { HostProvider } from "./providers";
import { HostStore } from "./store";
import { parseRemoteAttachments, resolveAttachments } from "./attachments";

// Streamed output is written in batches. Anything a user may need to act on
// (approvals, questions, errors, completion) is written immediately.
const FLUSH_MS = 120;
const BATCHED = new Set<string>([
  "message.delta",
  "reasoning.delta",
  "tool.updated",
  "agent.step",
  "status",
]);

const text = (value: unknown, label: string, max = 128): string => {
  if (
    typeof value !== "string" ||
    !value.trim() ||
    value.length > max ||
    value.includes("\0")
  )
    throw new Error(`Invalid ${label}`);
  return value;
};

function modelSettings(value: unknown): Record<string, string> {
  if (value == null) return {};
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Invalid model settings");
  const entries = Object.entries(value);
  if (
    entries.length > 20 ||
    entries.some(
      ([key, setting]) =>
        !/^[a-zA-Z][a-zA-Z0-9]{0,63}$/.test(key) ||
        typeof setting !== "string" ||
        setting.length > 128 ||
        setting.includes("\0"),
    )
  )
    throw new Error("Invalid model settings");
  return Object.fromEntries(entries) as Record<string, string>;
}

export function parseCommand(input: unknown): HostCommand {
  if (!input || typeof input !== "object" || Array.isArray(input))
    throw new Error("Invalid command");
  const v = input as Record<string, unknown>;
  const commandId = text(v.commandId, "command ID");
  if (v.type === "create") {
    if (
      !isRemoteProvider(v.harness) ||
      !RUNTIME_MODES.includes(v.runtimeMode as never)
    )
      throw new Error("Invalid provider or permission mode");
    if (
      v.autoWorktreeBranch !== undefined &&
      (v.worktreeCwd === undefined ||
        typeof v.autoWorktreeBranch !== "string" ||
        !/^mc\/[a-z0-9]{8}$/.test(v.autoWorktreeBranch))
    )
      throw new Error("Invalid automatically created worktree branch");
    return {
      type: "create",
      commandId,
      projectId: text(v.projectId, "project ID"),
      ...(v.worktreeCwd !== undefined
        ? { worktreeCwd: text(v.worktreeCwd, "working copy", 4096) }
        : {}),
      ...(v.autoWorktreeBranch !== undefined
        ? { autoWorktreeBranch: v.autoWorktreeBranch as string }
        : {}),
      harness: v.harness,
      model: text(v.model, "model", 200),
      ...(v.modelSettings !== undefined
        ? { modelSettings: modelSettings(v.modelSettings) }
        : {}),
      runtimeMode: v.runtimeMode as Session["runtimeMode"],
    };
  }
  const sessionId = text(v.sessionId, "session ID");
  if (v.type === "configure") {
    if (!RUNTIME_MODES.includes(v.runtimeMode as never))
      throw new Error("Invalid permission mode");
    return {
      type: "configure",
      commandId,
      sessionId,
      model: text(v.model, "model", 200),
      modelSettings: modelSettings(v.modelSettings),
      runtimeMode: v.runtimeMode as Session["runtimeMode"],
    };
  }
  if (v.type === "switchProvider") {
    if (!isRemoteProvider(v.harness) || !RUNTIME_MODES.includes(v.runtimeMode as never))
      throw new Error("Invalid provider or permission mode");
    if (!Number.isSafeInteger(v.expectedRevision) || Number(v.expectedRevision) < 0)
      throw new Error("Invalid expected session revision");
    return {
      type: "switchProvider",
      commandId,
      sessionId,
      expectedRevision: Number(v.expectedRevision),
      harness: v.harness,
      model: text(v.model, "model", 200),
      modelSettings: modelSettings(v.modelSettings),
      runtimeMode: v.runtimeMode as Session["runtimeMode"],
    };
  }
  if (v.type === "confirmProviderInspection") {
    if (!Number.isSafeInteger(v.expectedRevision) || Number(v.expectedRevision) < 0)
      throw new Error("Invalid expected session revision");
    return {
      type: "confirmProviderInspection", commandId, sessionId,
      expectedRevision: Number(v.expectedRevision),
    };
  }
  if (v.type === "compact") return { type: "compact", commandId, sessionId };
  if (v.type === "send" || v.type === "draft") {
    const attachments = parseRemoteAttachments(v.attachments);
    if (
      typeof v.text !== "string" ||
      v.text.length > 256_000 ||
      v.text.includes("\0") ||
      (!v.text.trim() &&
        attachments.length === 0 &&
        !(v.type === "send" && v.draftBlockId !== undefined))
    )
      throw new Error("Invalid prompt");
    if (
      v.type === "send" &&
      v.intent !== undefined &&
      !["default", "plan", "build"].includes(String(v.intent))
    )
      throw new Error("Invalid turn intent");
    if (
      v.planBlockId !== undefined &&
      (v.type !== "send" || v.intent !== "build")
    )
      throw new Error("Invalid plan build");
    return {
      type: v.type,
      commandId,
      sessionId,
      text: v.text,
      ...(attachments.length ? { attachments } : {}),
      ...(v.type === "send" && v.intent
        ? { intent: v.intent as "default" | "plan" | "build" }
        : {}),
      ...(v.type === "send" && v.draftBlockId !== undefined
        ? { draftBlockId: text(v.draftBlockId, "draft block ID") }
        : {}),
      ...(v.type === "send" && v.planBlockId !== undefined
        ? { planBlockId: text(v.planBlockId, "plan block ID") }
        : {}),
    };
  }
  if (v.type === "removeDraft")
    return {
      type: "removeDraft",
      commandId,
      sessionId,
      draftBlockId: text(v.draftBlockId, "draft block ID"),
    };
  const runId = text(v.runId, "run ID");
  if (v.type === "cancel")
    return { type: "cancel", commandId, sessionId, runId };
  if (!Number.isSafeInteger(v.requestId) || Number(v.requestId) < 0)
    throw new Error("Invalid request ID");
  const requestId = Number(v.requestId);
  if (v.type === "approve" && (v.decision === "allow" || v.decision === "deny"))
    return {
      type: "approve",
      commandId,
      sessionId,
      runId,
      requestId,
      decision: v.decision,
    };
  if (v.type === "answer") {
    const reply = v.reply as
      { kind?: string; answers?: unknown; custom?: unknown } | undefined;
    if (reply?.kind === "skipped")
      return {
        type: "answer",
        commandId,
        sessionId,
        runId,
        requestId,
        reply: { kind: "skipped" },
      };
    if (
      reply?.kind === "answered" &&
      reply.answers &&
      typeof reply.answers === "object" &&
      !Array.isArray(reply.answers)
    ) {
      const entries = Object.entries(reply.answers);
      if (
        entries.length > 50 ||
        entries.some(
          ([key, value]) =>
            key.length > 200 ||
            !Array.isArray(value) ||
            value.length > 50 ||
            value.some((x) => typeof x !== "string" || x.length > 10_000),
        )
      )
        throw new Error("Invalid question answers");
      if (
        reply.custom != null &&
        (typeof reply.custom !== "object" ||
          Array.isArray(reply.custom) ||
          Object.values(reply.custom).some(
            (x) => typeof x !== "string" || x.length > 10_000,
          ))
      )
        throw new Error("Invalid custom answers");
      return {
        type: "answer",
        commandId,
        sessionId,
        runId,
        requestId,
        reply: {
          kind: "answered",
          answers: Object.fromEntries(entries),
          ...(reply.custom
            ? { custom: reply.custom as Record<string, string> }
            : {}),
        },
      };
    }
  }
  throw new Error("Unsupported command");
}

export class HostEngine {
  private switchingProjects = new Set<string>();
  private running = new Map<
    string,
    { runId: string; done: Promise<void>; cancelled: boolean; persistenceFailed: boolean; accepted: boolean }
  >();
  /** Running sessions, including streamed events not yet written to disk. */
  private live = new Map<
    string,
    {
      value: HostSession;
      events: HarnessEvent[];
      timer?: ReturnType<typeof setTimeout>;
    }
  >();
  private retryTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private closing = false;

  constructor(
    readonly store: HostStore,
    private readonly providers: Partial<Record<RemoteProvider, HostProvider>>,
  ) {
    // Provider dispatch is not transactional with SQLite. Never replay a send
    // automatically after a crash; its external effects may already exist.
    for (const value of store.sessions()) {
      let recovered = value;
      if (value.status === "running") {
        recovered = this.save(
          this.settled(
            value,
            "interrupted",
            "Host restarted. This turn was interrupted; inspect its work before continuing.",
            value.updatedAt,
          ),
          { type: "interrupted" },
        );
      }
      const delivery = recovered.session.providerContext?.delivery;
      if (delivery && delivery.status !== "accepted" && delivery.status !== "uncertain") {
        recovered = this.save({
          ...recovered,
          session: this.contextStatus(delivery.requestSubmitted
            ? recoverSubmittedProviderDelivery(recovered.session, delivery.switchId)
            : failProviderDelivery(recovered.session, delivery.switchId), delivery.switchId, "uncertain"),
        }, { type: "providerContext.recovered", switchId: delivery.switchId });
      }
      if (recovered.session.providerSessionId &&
        !requiresFreshProviderBinding(recovered.session, recovered.session.harness, recovered.session.cwd))
        this.provider(recovered.session.harness).bind(
          recovered.session.id,
          recovered.session.providerSessionId,
          recovered.session.cwd,
        );
    }
  }

  async openProject(path: string) {
    if (!isAbsolute(path) || path.includes("\0"))
      throw new Error("Choose an absolute directory path on the host");
    const cwd = await realpath(path);
    if (!(await stat(cwd)).isDirectory())
      throw new Error("Project path is not a directory");
    return this.store.addProject(cwd, basename(cwd));
  }

  async withIdleProject<T>(
    projectId: string,
    action: () => Promise<T>,
  ): Promise<T> {
    if (this.switchingProjects.has(projectId))
      throw new Error("A branch switch is already in progress");
    if (
      this.store
        .summaries(projectId)
        .some((session) => session.status === "running")
    )
      throw new Error(
        "Wait for running host sessions before switching branches",
      );
    this.switchingProjects.add(projectId);
    try {
      return await action();
    } finally {
      this.switchingProjects.delete(projectId);
    }
  }

  private provider(id: string): HostProvider {
    const provider = this.providers[id as RemoteProvider];
    if (!provider) throw new Error(`${id} is not available on this host`);
    return provider;
  }

  private contextDirectory(sessionId: string): string {
    return join(dirname(this.store.attachmentDir), "context-history", sessionId);
  }

  private portableHistory(session: Session, switchId: string, currentRequest: string, binding?: ProviderBinding, assetSnapshots?: ContextAssetSnapshot[], attachmentTokens = 0): PortableContext {
    const context = buildPortableContext(session, {
      afterBlockId: binding?.deliveredThroughBlockId,
      currentRequest,
      windowTokens: resolveModel(session.harness, session.model).contextWindow ?? binding?.contextWindow,
      occupiedTokens: binding?.contextUsed,
      attachmentTokens,
      assetSnapshots,
    });
    if (context.omitted.some((item) => item.reason === "budget")) {
      const directory = this.contextDirectory(session.id);
      mkdirSync(directory, { recursive: true, mode: 0o700 });
      const path = join(directory, `${createHash("sha256").update(switchId).digest("hex")}.md`);
      writeFileSync(path, buildPortableContextSnapshot(session, context.throughBlockId, assetSnapshots), { mode: 0o600 });
      context.retrievalPath = path;
    }
    return context;
  }

  private contextStatus(session: Session, switchId: string, status: "imported" | "accepted" | "uncertain"): Session {
    const delivery = session.providerContext?.delivery;
    if (!delivery || delivery.switchId !== switchId) return session;
    const mode = delivery.mode === "native" ? "native messages" : "attributed history";
    const detail = `Delivered ${delivery.includedBlockIds.length} transcript items as ${mode}. Omitted ${delivery.omittedBlockIds.length}. Historical attachments remain file references.`;
    const text = status === "accepted"
      ? `Continued with shared history. ${detail}`
      : status === "imported"
        ? `${detail} The current request awaits acceptance.`
        : delivery.needsInspection
          ? "The provider request may already have run. Inspect its work, then confirm inspection before continuing."
          : "The provider did not acknowledge this transfer. The next turn will use a fresh conversation with shared history. You can also return to the source provider.";
    return {
      ...session,
      blocks: session.blocks.map((block) => block.id === `${switchId}-context`
        ? { ...block, text, handoff: { ...block.handoff!, status: status === "accepted" ? "ready" : "preparing", pending: status !== "accepted" } }
        : block),
    };
  }

  private updateProviderState(id: string, runId: string, update: (session: Session) => Session, event: unknown): boolean {
    const live = this.live.get(id);
    if (!live || live.value.runId !== runId || live.value.status !== "running") return false;
    try {
      const session = update(live.value.session);
      const changed = session !== live.value.session;
      if (changed) {
        // Retain evidence in memory even if its receipt cannot be saved yet.
        live.value = { ...live.value, session };
      }
      this.flush(id);
      if (changed) live.value = this.save(live.value, event);
      return true;
    } catch (error) {
      const active = this.running.get(id);
      if (active) active.persistenceFailed = true;
      void this.provider(live.value.session.harness).stop(id);
      throw error;
    }
  }

  private save(value: HostSession, event: unknown): HostSession {
    return this.store.transaction(() =>
      this.store.save(
        { ...value, revision: value.revision + 1, updatedAt: Date.now() },
        event,
      ),
    );
  }

  updateSession(id: string, patch: Parameters<HostStore["updateSession"]>[1]) {
    this.flush(id);
    const summary = this.store.updateSession(id, patch);
    const live = this.live.get(id);
    if (live) live.value = this.store.session(id);
    return summary;
  }

  private flush(id: string): void {
    const live = this.live.get(id);
    if (!live) return;
    clearTimeout(live.timer);
    live.timer = undefined;
    if (!live.events.length) return;
    const events = live.events;
    live.value = this.save(live.value, { type: "events", events });
    live.events = [];
  }

  private scheduledFlush(id: string, provider: HostProvider): void {
    try {
      this.flush(id);
    } catch (error) {
      const active = this.running.get(id);
      if (active) active.persistenceFailed = true;
      console.error(
        "Session persistence failed; stopping its provider:",
        error instanceof Error ? error.message : "unknown error",
      );
      void provider.stop(id);
    }
  }

  private retrySettlement(
    id: string,
    runId: string,
    provider: HostProvider,
  ): void {
    if (this.closing || this.retryTimers.has(id)) return;
    const timer = setTimeout(() => {
      this.retryTimers.delete(id);
      void (async () => {
        try {
          await provider.stop(id);
          this.flush(id);
          const persisted = this.store.session(id);
          let latest = persisted.runId === runId && persisted.status === "running" ? this.live.get(id)?.value ?? persisted : persisted;
          if (latest.runId === runId && latest.status === "running") {
            const delivery = latest.session.providerContext?.delivery;
            if (this.running.get(id)?.accepted) {
              if (delivery) latest = { ...latest, session: this.contextStatus(acceptProviderDelivery(latest.session, delivery.switchId), delivery.switchId, "accepted") };
              latest = { ...latest, session: settleProviderBinding(latest.session, latest.session.harness, latest.session.cwd) };
            } else if (delivery && delivery.status !== "accepted") {
              const recovered = delivery.requestSubmitted
                ? recoverSubmittedProviderDelivery(latest.session, delivery.switchId)
                : failProviderDelivery(latest.session, delivery.switchId);
              latest = { ...latest, session: this.contextStatus(recovered, delivery.switchId, "uncertain") };
              if (!recovered.providerContext?.delivery?.needsInspection)
                await (provider.forget ?? provider.stop).call(provider, id);
            }
            latest = this.save(
              this.settled(
                latest,
                "interrupted",
                "Session storage failed during this turn. Inspect its work before continuing.",
                latest.updatedAt,
              ),
              { type: "interrupted", reason: "persistence failure" },
            );
          }
          this.live.delete(id);
          this.running.delete(id);
          if (latest.session.providerSessionId &&
            !requiresFreshProviderBinding(latest.session, latest.session.harness, latest.session.cwd))
            provider.bind(
              id,
              latest.session.providerSessionId,
              latest.session.cwd,
            );
        } catch (error) {
          console.error(
            "Retrying session persistence:",
            error instanceof Error ? error.message : "unknown error",
          );
          this.retrySettlement(id, runId, provider);
        }
      })();
    }, 1_000);
    timer.unref?.();
    this.retryTimers.set(id, timer);
  }

  command(raw: unknown): CommandReceipt {
    if (this.closing) throw new Error("Host is stopping");
    const command = parseCommand(raw);
    const signature = createHash("sha256")
      .update(JSON.stringify(command))
      .digest("hex");
    const previous = this.store.receipt(command.commandId, signature);
    if (previous) return previous;
    // Commands apply to the latest state, including batched stream output.
    if (command.type !== "create") this.flush(command.sessionId);
    let effect: ((saved: HostSession) => void) | undefined;
    const { receipt, saved } = this.store.transaction(() => {
      let value: HostSession;
      if (command.type === "create") {
        const project = this.store.project(command.projectId);
        if (this.switchingProjects.has(project.id))
          throw new Error("Wait for the branch switch to finish");
        this.provider(command.harness);
        const cwd = resolveHostWorktree(project.cwd, command.worktreeCwd);
        const now = Date.now();
        value = {
          projectId: project.id,
          autoWorktreeBranch: command.autoWorktreeBranch,
          revision: 0,
          status: "idle",
          createdAt: now,
          updatedAt: now,
          session: {
            id: randomUUID(),
            cwd,
            harness: command.harness,
            model: command.model,
            runtimeMode: command.runtimeMode,
            modelSettings: command.modelSettings ?? {},
            title: "New remote session",
            ...(command.autoWorktreeBranch
              ? { branch: command.autoWorktreeBranch, worktreeCwd: cwd }
              : {}),
            blocks: [],
          },
        };
      } else {
        value = this.store.session(command.sessionId);
        if (
          (command.type === "send" || command.type === "compact") &&
          this.switchingProjects.has(value.projectId)
        )
          throw new Error("Wait for the branch switch to finish");
        const provider = this.provider(value.session.harness);
        if (command.type === "confirmProviderInspection") {
          if (value.status === "running") throw new Error("Wait for host storage to reconcile before confirming inspection");
          if (value.revision !== command.expectedRevision)
            throw new Error("Session changed on the host. Reload it before confirming inspection");
          if (!value.session.providerContext?.delivery?.needsInspection)
            throw new Error("This session does not need inspection confirmation");
          value = { ...value, session: confirmProviderDeliveryInspection(value.session) };
        } else if (value.session.providerContext?.delivery?.needsInspection &&
          (command.type === "send" || command.type === "compact" || command.type === "switchProvider")) {
          throw new Error("Inspect the interrupted provider request and confirm inspection before continuing");
        } else if (command.type === "switchProvider") {
          if (value.status === "running")
            throw new Error("Wait for the current turn before changing providers");
          if (value.revision !== command.expectedRevision)
            throw new Error("Session changed on the host. Reload it before changing providers");
          this.provider(command.harness);
          let session = value.session;
          const source = providerBinding(session, session.harness, session.cwd);
          if (source) session = rememberProviderBinding(session, source);
          const plan = planComposerSwitch(session, command.harness);
          const target = providerBinding(session, command.harness, session.cwd);
          const resumable = this.provider(command.harness).contextTransferCapabilities?.resumedAppend;
          session = {
            ...session,
            harness: command.harness,
            model: command.model,
            modelSettings: command.modelSettings,
            runtimeMode: command.runtimeMode,
            context: undefined,
            providerSessionId: resumable ? target?.providerSessionId : undefined,
            providerAccountId: undefined,
          };
          if (plan.kind === "arm") session = { ...session, pendingSwitch: plan.pending };
          else if (plan.kind === "revert") session = {
            ...session,
            pendingSwitch: undefined,
            providerSessionId: plan.restoreProviderSessionId,
          };
          else if (plan.kind === "empty") session = { ...session, pendingSwitch: undefined };
          value = { ...value, session };
        } else if (command.type === "configure") {
          if (value.status === "running")
            throw new Error(
              "Wait for the current turn before changing settings",
            );
          value = {
            ...value,
            session: {
              ...value.session,
              model: command.model,
              modelSettings: command.modelSettings,
              runtimeMode: command.runtimeMode,
            },
          };
        } else if (command.type === "draft") {
          if (
            value.status === "running" ||
            value.session.blocks.some((block) => block.draft)
          )
            throw new Error("This session cannot save another draft right now");
          const attachments = resolveAttachments(
            this.store,
            command.attachments ?? [],
          );
          value = {
            ...value,
            session: {
              ...value.session,
              title: value.session.blocks.length
                ? value.session.title
                : titleFromPrompt(
                    command.text,
                    value.session.harness,
                    attachments,
                  ),
              blocks: [
                ...value.session.blocks,
                {
                  id: command.commandId,
                  role: "user",
                  text: command.text,
                  ...(attachments.length ? { attachments } : {}),
                  draft: true,
                },
              ],
            },
          };
        } else if (command.type === "removeDraft") {
          const draft = value.session.blocks.find(
            (block) => block.id === command.draftBlockId && block.draft,
          );
          if (!draft) throw new Error("Draft not found");
          value = {
            ...value,
            session: {
              ...value.session,
              blocks: value.session.blocks.filter(
                (block) => block.id !== draft.id,
              ),
            },
          };
        } else if (command.type === "send" || command.type === "compact") {
          if (value.status === "running")
            throw new Error("This session is already running");
          if (command.type === "compact" && !provider.compact)
            throw new Error(
              "Context compaction is unavailable for this provider",
            );
          if (command.type === "compact" && (value.session.pendingSwitch ||
            requiresFreshProviderBinding(value.session, value.session.harness, value.session.cwd)))
            throw new Error("Send a turn with shared history before compacting this provider");
          const draft =
            command.type === "send" && command.draftBlockId
              ? value.session.blocks.find(
                  (block) => block.id === command.draftBlockId && block.draft,
                )
              : undefined;
          if (command.type === "send" && command.draftBlockId && !draft)
            throw new Error("Draft not found");
          const plan =
            command.type === "send" && command.planBlockId
              ? value.session.blocks.find(
                  (block) =>
                    block.id === command.planBlockId && block.role === "plan",
                )
              : undefined;
          if (
            command.type === "send" &&
            command.planBlockId &&
            (!plan ||
              !plan.text.trim() ||
              plan.streaming ||
              plan.plan?.status === "building" ||
              plan.plan?.status === "built")
          )
            throw new Error("Plan is not ready to build");
          const attachments =
            command.type === "send"
              ? (draft?.attachments ??
                resolveAttachments(this.store, command.attachments ?? []))
              : [];
          const runId = randomUUID();
          const pendingSwitch = command.type === "send" ? value.session.pendingSwitch : undefined;
          const uncertain = command.type === "send" && requiresFreshProviderBinding(value.session, value.session.harness, value.session.cwd);
          const switching = pendingSwitch && pendingSwitch.from !== value.session.harness;
          let transfer: { context: PortableContext; fallbackContext: PortableContext; switchId: string; fresh: boolean } | undefined;
          if (command.type === "send" && (switching || uncertain)) {
            const switchId = command.commandId;
            const binding = providerBinding(value.session, value.session.harness, value.session.cwd);
            const canResume = !uncertain && provider.contextTransferCapabilities?.resumedAppend === true && canResumeProviderBinding(value.session, binding);
            const currentRequest = command.text;
            const historicalAttachments = historicalContextAttachments(value.session);
            const assetSnapshots = snapshotHostContextAssets(join(this.contextDirectory(value.session.id), "assets"), historicalAttachments);
            const attachmentTokens = currentAttachmentTokens(attachments);
            const context = this.portableHistory(value.session, switchId, currentRequest, canResume ? binding : undefined, assetSnapshots, attachmentTokens);
            const fallbackContext = canResume
              ? this.portableHistory(value.session, `${switchId}-fallback`, currentRequest, undefined, assetSnapshots, attachmentTokens)
              : context;
            const from = pendingSwitch?.from ?? value.session.providerContext!.delivery!.from;
            value = {
              ...value,
              session: updateProviderHandoff(beginProviderDelivery({
                ...value.session,
                providerSessionId: canResume ? binding.providerSessionId : undefined,
                blocks: [...value.session.blocks, {
                  id: `${command.commandId}-context`,
                  role: "handoff",
                  text: `Preparing shared history. Selected ${context.items.length} transcript items and omitted ${context.omitted.length}. Historical attachments remain file references.`,
                  handoff: { from, to: value.session.harness, status: "preparing", pending: true },
                }],
              }, {
                switchId,
                from,
                to: value.session.harness,
                cwd: value.session.cwd,
                currentUserBlockId: command.commandId,
                sourceThroughBlockId: context.throughBlockId,
                includedBlockIds: context.items.map((item) => item.sourceBlockId),
                omittedBlockIds: context.omitted.map((item) => item.id),
                ...(canResume ? { targetProviderSessionId: binding.providerSessionId } : {}),
              }), switchId, {
                historicalAttachments: context.items.reduce((count, item) => count + (item.attachments?.length ?? 0), 0),
                retrievalPath: context.retrievalPath,
              }),
            };
            transfer = { context, fallbackContext, switchId, fresh: !canResume };
          }
          const firstTurn =
            command.type === "send" &&
            !value.session.blocks.some((block) => !block.draft);
          const placeholderTitle =
            value.session.title === "New remote session" ||
            canReplaceSessionTitle(
              value.session.title,
              value.session.harness,
              HARNESS_LABEL[value.session.harness],
            );
          const model = resolveModel(
            value.session.harness,
            value.session.model,
          );
          value = {
            ...value,
            status: "running",
            runId,
            session: {
              ...value.session,
              busy: true,
              pendingQuestion: undefined,
              title:
                firstTurn && placeholderTitle
                  ? titleFromPrompt(
                      command.text,
                      value.session.harness,
                      attachments,
                    )
                  : value.session.title,
              blocks: [
                ...value.session.blocks
                  .filter((block) => !block.draft)
                  .map((block) =>
                    block === plan
                      ? {
                          ...block,
                          plan: {
                            ...(block.plan ?? { status: "ready" as const }),
                            status: "building" as const,
                            approvedText: block.text,
                          },
                        }
                      : block,
                  ),
                {
                  id: command.commandId,
                  role: "user",
                  text: command.type === "compact" ? "/compact" : command.text,
                  ...(attachments.length ? { attachments } : {}),
                  startedAt: Date.now(),
                  turnModel: {
                    harness: value.session.harness,
                    id: value.session.model,
                    name:
                      model.id === value.session.model
                        ? model.name
                        : value.session.model.replace(/^[^:]+:/, ""),
                  },
                },
              ],
            },
          };
          if (transfer) value = { ...value, session: markProviderRequestSubmitted(value.session, transfer.switchId) };
          effect = (saved) => {
            this.run(
              saved,
              command.type === "compact" ? null : command.text,
              command.type === "send" ? command.intent : undefined,
              attachments,
              transfer,
            );
            if (firstTurn && command.type === "send") {
              this.generateFirstTurnNames(
                saved,
                command.text,
                placeholderTitle,
              );
            }
          };
        } else {
          if (value.runId !== command.runId || value.status !== "running")
            throw new Error(
              "This request belongs to a finished or replaced turn",
            );
          if (command.type === "cancel") {
            effect = () => {
              const active = this.running.get(command.sessionId);
              if (active) active.cancelled = true;
              void provider
                .cancel(command.sessionId)
                .catch(() => provider.stop(command.sessionId));
            };
          } else if (command.type === "approve") {
            const pending = value.session.blocks.some(
              (block) =>
                block.approval?.requestId === command.requestId &&
                !block.approval.decided,
            );
            if (!pending) throw new Error("Approval is already resolved");
            value = {
              ...value,
              session: applyHarnessEvent(value.session, {
                type: "approval.resolved",
                requestId: command.requestId,
                decision: command.decision,
              }),
            };
            effect = () =>
              provider.approve(
                command.sessionId,
                command.requestId,
                command.decision,
              );
          } else {
            if (value.session.pendingQuestion?.requestId !== command.requestId)
              throw new Error("Question is already resolved");
            value = {
              ...value,
              session: { ...value.session, pendingQuestion: undefined },
            };
            effect = () =>
              provider.answer(
                command.sessionId,
                command.requestId,
                command.reply,
              );
          }
        }
      }
      const saved = this.store.save(
        {
          ...value,
          revision: value.revision + 1,
          // Creation already initialized both timestamps from the same clock read.
          updatedAt: command.type === "create" ? value.updatedAt : Date.now(),
        },
        { type: "command", command },
      );
      const result = {
        commandId: command.commandId,
        sessionId: saved.session.id,
        revision: saved.revision,
      };
      this.store.recordReceipt(signature, result);
      return { receipt: result, saved };
    });
    const live = this.live.get(saved.session.id);
    if (live) live.value = saved;
    // A receipt means durable host acceptance, not provider completion.
    effect?.(saved);
    return receipt;
  }

  private generateFirstTurnNames(
    value: HostSession,
    message: string,
    generateTitle: boolean,
  ): void {
    const provider = this.provider(value.session.harness);
    const { id, cwd, harness, title } = value.session;
    if (generateTitle && provider.generateTitle) {
      void provider
        .generateTitle({ sessionId: id, cwd, message })
        .then((generated) => {
          if (!generated) return;
          this.flush(id);
          const current = this.store.session(id);
          if (current.session.title !== title) return;
          const saved = this.save(
            {
              ...current,
              session: {
                ...current.session,
                title: formatSessionTitle(harness, generated.title),
              },
            },
            { type: "session.generatedTitle" },
          );
          const live = this.live.get(id);
          if (live) live.value = saved;
        })
        .catch((error) =>
          console.debug("[monocode] remote session title", error),
        );
    }
    const temporary = value.autoWorktreeBranch;
    if (temporary && provider.generateBranchName) {
      void provider
        .generateBranchName(cwd, message)
        .then(async (fragment) => {
          const branch = fragment ? namedWorktreeBranch(fragment) : null;
          if (!branch) return;
          // A title/branch request may finish after the conversation was deleted.
          const currentBeforeRename = this.store.session(id);
          if (currentBeforeRename.autoWorktreeBranch !== temporary) return;
          const project = this.store.project(value.projectId);
          await renameHostWorktreeBranch(project.cwd, cwd, temporary, branch,
            () => this.store.session(id).autoWorktreeBranch === temporary);
          this.flush(id);
          const current = this.store.session(id);
          const saved = this.save(
            {
              ...current,
              autoWorktreeBranch: undefined,
              session: { ...current.session, branch },
            },
            { type: "session.generatedBranch", branch },
          );
          const live = this.live.get(id);
          if (live) live.value = saved;
        })
        .catch((error) =>
          console.debug("[monocode] remote worktree branch", error),
        );
    }
  }

  private run(
    value: HostSession,
    prompt: string | null,
    intent?: "default" | "plan" | "build",
    attachments: Session["blocks"][number]["attachments"] = [],
    transfer?: { context: PortableContext; fallbackContext: PortableContext; switchId: string; fresh: boolean },
  ): void {
    const { session, runId } = value;
    const provider = this.provider(session.harness);
    const active = { runId: runId!, done: Promise.resolve(), cancelled: false, persistenceFailed: false, accepted: false };
    this.running.set(session.id, active);
    this.live.set(session.id, { value, events: [] });
    active.done = Promise.resolve()
      .then(async () => {
        let error: string | undefined;
        try {
          if (!this.closing && !active.cancelled) {
            if (session.pendingSwitch && session.pendingSwitch.from !== session.harness)
              await this.provider(session.pendingSwitch.from).stop(session.id);
            if (transfer?.fresh) await (provider.forget ?? provider.stop).call(provider, session.id);
            else if (session.providerSessionId) provider.bind(session.id, session.providerSessionId, session.cwd);
            const input: HarnessSessionInput = {
              sessionId: session.id,
              cwd: session.cwd,
              model: session.model,
              modelSettings: session.modelSettings,
              runtimeMode: session.runtimeMode,
              intent,
              onEvent: (event) => this.event(session.id, runId!, event),
            };
            if (prompt === null) await provider.compact!(input);
            else {
              const turn: SendTurnInput = {
                ...input,
                text: prompt,
                attachments: attachments?.map((file) =>
                  isVisionImage(file.mimeType) &&
                  file.path &&
                  file.size <= 20 * 1024 * 1024
                    ? {
                        ...file,
                        data: readFileSync(file.path).toString("base64"),
                      }
                    : file,
                ),
                onAccepted: () => {
                  if (this.live.get(session.id)?.value.runId !== runId) return;
                  active.accepted = true;
                  try {
                    this.updateProviderState(session.id, runId!, (current) => {
                      if (!transfer) return current;
                      return this.contextStatus(acceptProviderDelivery(current, transfer.switchId), transfer.switchId, "accepted");
                    }, { type: "providerContext.accepted", switchId: transfer?.switchId });
                  } catch {
                    // The receipt failure must not escape the provider's stdout listener.
                    // Settlement retries storage with the retained acknowledgment.
                  }
                },
                ...(transfer ? {
                  contextTransfer: {
                    context: transfer.context,
                    fallbackContext: transfer.fallbackContext,
                    onDelivered: (receipt) => {
                      if (this.live.get(session.id)?.value.runId !== runId) return;
                      // Inline history arrives in the acknowledged current request.
                      if (receipt.mode === "inline") active.accepted = true;
                      try {
                        this.updateProviderState(session.id, runId!, (current) => {
                          const knownItems = [...transfer.context.items, ...transfer.fallbackContext.items];
                          const includedIds = receipt.includedIds?.map((id) => knownItems.find((item) => item.id === id)?.sourceBlockId ?? id);
                          let delivered = markProviderContextDelivered(current, transfer.switchId, receipt.mode, receipt.providerSessionId, {
                            ...(includedIds ? { includedBlockIds: includedIds } : {}),
                            ...(receipt.throughBlockId ? { sourceThroughBlockId: receipt.throughBlockId } : {}),
                          });
                          if (receipt.includedIds && delivered.providerContext?.delivery) delivered = {
                            ...delivered,
                            providerContext: {
                              ...delivered.providerContext,
                              delivery: {
                                ...delivered.providerContext.delivery,
                                includedBlockIds: includedIds!,
                                omittedBlockIds: receipt.omittedIds ?? delivered.providerContext.delivery.omittedBlockIds,
                                sourceThroughBlockId: receipt.throughBlockId ?? delivered.providerContext.delivery.sourceThroughBlockId,
                              },
                            },
                          };
                          delivered = updateProviderHandoff(delivered, transfer.switchId, {
                            omitted: delivered.providerContext!.delivery!.omittedBlockIds.length,
                          });
                          return this.contextStatus(delivered, transfer.switchId, "imported");
                        }, { type: "providerContext.delivered", switchId: transfer.switchId, mode: receipt.mode });
                      } catch (error) {
                        if (receipt.mode !== "inline") throw error;
                      }
                    },
                  },
                } : {}),
              };
              await provider.send(prepareContextTransferInput(turn, provider.contextTransferCapabilities));
            }
          }
        } catch (reason) {
          error = reason instanceof Error ? reason.message : String(reason);
        }
        // Keep the session running until the old process has stopped. Otherwise
        // a follow-up can race cleanup and have its newly spawned child killed.
        await provider.stop(session.id);
        this.flush(session.id);
        const stored = this.store.session(session.id);
        let latest = stored.runId === runId && stored.status === "running" ? this.live.get(session.id)?.value ?? stored : stored;
        if (latest.runId === runId && latest.status === "running") {
          if (active.accepted) {
            if (transfer) latest = { ...latest, session: this.contextStatus(acceptProviderDelivery(latest.session, transfer.switchId), transfer.switchId, "accepted") };
            latest = { ...latest, session: settleProviderBinding(latest.session, session.harness, session.cwd) };
          }
          else if (transfer) {
            const recovered = latest.session.providerContext?.delivery?.requestSubmitted
              ? recoverSubmittedProviderDelivery(latest.session, transfer.switchId)
              : failProviderDelivery(latest.session, transfer.switchId);
            latest = { ...latest, session: this.contextStatus(recovered, transfer.switchId, "uncertain") };
            if (!recovered.providerContext?.delivery?.needsInspection)
              await (provider.forget ?? provider.stop).call(provider, session.id);
          }
          const message = this.closing
            ? "Host stopped. This turn was interrupted."
            : active.persistenceFailed
              ? "Session storage failed during this turn. Inspect its work before continuing."
              : active.cancelled
              ? "Stopped by you."
              : error;
          this.save(
            this.settled(
              latest,
              this.closing || active.persistenceFailed ? "interrupted" : "idle",
              message,
            ),
            { type: "settled", error, cancelled: active.cancelled },
          );
        }
        this.live.delete(session.id);
        this.running.delete(session.id);
        // stop/forget releases callbacks and native resources; bind only retained
        // provider conversation identity for an explicit future follow-up.
        const persisted = this.store.session(session.id).session;
        if (persisted.providerSessionId &&
          !requiresFreshProviderBinding(persisted, persisted.harness, persisted.cwd))
          provider.bind(session.id, persisted.providerSessionId, persisted.cwd);
      })
      .catch((error) => {
        clearTimeout(this.live.get(session.id)?.timer);
        console.error(
          "Session persistence failed; stopping its provider:",
          error instanceof Error ? error.message : "unknown error",
        );
        void provider.stop(session.id);
        this.retrySettlement(session.id, runId!, provider);
      });
  }

  private event(id: string, runId: string, event: HarnessEvent): void {
    const live = this.live.get(id);
    if (!live || live.value.runId !== runId || live.value.status !== "running")
      return;
    let session = applyHarnessEvent(live.value.session, event);
    if (event.type === "session.providerBound")
      session = recordProviderBound(session, session.harness, session.cwd, event.providerSessionId);
    if (event.type === "context")
      session = recordProviderContextUsage(session, session.harness, session.cwd, event);
    if (session === live.value.session) return;
    live.value = { ...live.value, session };
    live.events.push(event);
    if (!BATCHED.has(event.type))
      this.scheduledFlush(id, this.provider(session.harness));
    else
      live.timer ??= setTimeout(
        () => this.scheduledFlush(id, this.provider(session.harness)),
        FLUSH_MS,
      );
  }

  private settled(
    value: HostSession,
    status: "idle" | "interrupted",
    message?: string,
    endedAt = Date.now(),
  ): HostSession {
    const stopped = stopStreaming(value.session, endedAt);
    const session = {
      ...stopped,
      blocks: stopped.blocks.map((block) =>
        block.role === "plan" && block.plan?.status === "building"
          ? {
              ...block,
              plan: {
                ...block.plan,
                status:
                  status === "idle" && !message
                    ? ("built" as const)
                    : ("ready" as const),
              },
            }
          : block,
      ),
    };
    if (message)
      session.blocks.push({
        id: randomUUID(),
        role: "system",
        text: message,
        notice: status === "interrupted" || message === "Stopped by you." ? "interrupt" : "error",
        streaming: false,
      });
    return { ...value, status, session };
  }

  async close(): Promise<void> {
    this.closing = true;
    for (const timer of this.retryTimers.values()) clearTimeout(timer);
    this.retryTimers.clear();
    await Promise.all(
      [...this.running.keys()].map((id) =>
        this.provider(this.store.session(id).session.harness).stop(id),
      ),
    );
    await Promise.all([...this.running.values()].map((active) => active.done));
  }
}
