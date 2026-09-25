import { createHash, randomUUID } from "node:crypto";
import { realpath, stat } from "node:fs/promises";
import { basename, isAbsolute } from "node:path";
import { applyHarnessEvent } from "../src/integrations/harness/core/apply";
import type { HarnessEvent } from "../src/integrations/harness/core/types";
import {
  RUNTIME_MODES,
  type Session,
} from "../src/features/sessions/model/session";
import {
  isRemoteProvider,
  type HostCommand,
  type HostSession,
  type CommandReceipt,
  type RemoteProvider,
} from "../src/features/connections/model/protocol";
import type { HostProvider } from "./providers";
import { HostStore } from "./store";

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
    return {
      type: "create",
      commandId,
      projectId: text(v.projectId, "project ID"),
      harness: v.harness,
      model: text(v.model, "model", 200),
      runtimeMode: v.runtimeMode as Session["runtimeMode"],
    };
  }
  const sessionId = text(v.sessionId, "session ID");
  if (v.type === "send")
    return {
      type: "send",
      commandId,
      sessionId,
      text: text(v.text, "prompt", 256_000),
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
  private running = new Map<
    string,
    { runId: string; done: Promise<void>; cancelled: boolean }
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
  private closing = false;

  constructor(
    readonly store: HostStore,
    private readonly providers: Partial<Record<RemoteProvider, HostProvider>>,
  ) {
    // Provider dispatch is not transactional with SQLite. Never replay a send
    // automatically after a crash; its external effects may already exist.
    for (const value of store.sessions()) {
      if (value.status === "running") {
        this.save(
          this.settled(
            value,
            "interrupted",
            "Host restarted. This turn was interrupted; inspect its work before continuing.",
          ),
          { type: "interrupted" },
        );
      }
      if (value.session.providerSessionId)
        this.provider(value.session.harness).bind(
          value.session.id,
          value.session.providerSessionId,
          value.session.cwd,
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

  private provider(id: string): HostProvider {
    const provider = this.providers[id as RemoteProvider];
    if (!provider) throw new Error(`${id} is not available on this host`);
    return provider;
  }

  private save(value: HostSession, event: unknown): HostSession {
    return this.store.transaction(() =>
      this.store.save(
        { ...value, revision: value.revision + 1, updatedAt: Date.now() },
        event,
      ),
    );
  }

  private flush(id: string): void {
    const live = this.live.get(id);
    if (!live) return;
    clearTimeout(live.timer);
    live.timer = undefined;
    if (!live.events.length) return;
    const events = live.events;
    live.events = [];
    live.value = this.save(live.value, { type: "events", events });
  }

  private scheduledFlush(id: string, provider: HostProvider): void {
    try {
      this.flush(id);
    } catch (error) {
      console.error(
        "Session persistence failed; stopping its provider:",
        error instanceof Error ? error.message : "unknown error",
      );
      void provider.stop(id);
    }
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
        this.provider(command.harness);
        value = {
          projectId: project.id,
          revision: 0,
          status: "idle",
          updatedAt: Date.now(),
          session: {
            id: randomUUID(),
            cwd: project.cwd,
            harness: command.harness,
            model: command.model,
            runtimeMode: command.runtimeMode,
            modelSettings: {},
            title: "New remote session",
            blocks: [],
          },
        };
      } else {
        value = this.store.session(command.sessionId);
        const provider = this.provider(value.session.harness);
        if (command.type === "send") {
          if (value.status === "running")
            throw new Error("This session is already running");
          const runId = randomUUID();
          value = {
            ...value,
            status: "running",
            runId,
            session: {
              ...value.session,
              busy: true,
              pendingQuestion: undefined,
              title: value.session.blocks.length
                ? value.session.title
                : command.text.trim().split("\n")[0].slice(0, 72),
              blocks: [
                ...value.session.blocks,
                { id: command.commandId, role: "user", text: command.text },
              ],
            },
          };
          effect = (saved) => this.run(saved, command.text);
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
        { ...value, revision: value.revision + 1, updatedAt: Date.now() },
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

  private run(value: HostSession, prompt: string): void {
    const { session, runId } = value;
    const provider = this.provider(session.harness);
    const active = { runId: runId!, done: Promise.resolve(), cancelled: false };
    this.running.set(session.id, active);
    this.live.set(session.id, { value, events: [] });
    active.done = Promise.resolve()
      .then(async () => {
        let error: string | undefined;
        try {
          if (!this.closing && !active.cancelled) {
            await provider.send({
              sessionId: session.id,
              cwd: session.cwd,
              model: session.model,
              modelSettings: session.modelSettings,
              runtimeMode: session.runtimeMode,
              text: prompt,
              onEvent: (event) => this.event(session.id, runId!, event),
            });
          }
        } catch (reason) {
          error = reason instanceof Error ? reason.message : String(reason);
        }
        // Keep the session running until the old process has stopped. Otherwise
        // a follow-up can race cleanup and have its newly spawned child killed.
        await provider.stop(session.id);
        this.flush(session.id);
        this.live.delete(session.id);
        const latest = this.store.session(session.id);
        if (latest.runId === runId) {
          const message = this.closing
            ? "Host stopped. This turn was interrupted."
            : active.cancelled
              ? "Stopped by you."
              : error;
          this.save(
            this.settled(
              latest,
              this.closing ? "interrupted" : "idle",
              message,
            ),
            { type: "settled", error, cancelled: active.cancelled },
          );
        }
        this.running.delete(session.id);
        // stop/forget releases callbacks and native resources; bind only retained
        // provider conversation identity for an explicit future follow-up.
        const persisted = this.store.session(session.id).session;
        if (persisted.providerSessionId)
          provider.bind(session.id, persisted.providerSessionId, persisted.cwd);
      })
      .catch((error) => {
        clearTimeout(this.live.get(session.id)?.timer);
        this.live.delete(session.id);
        this.running.delete(session.id);
        console.error(
          "Session persistence failed; stopping its provider:",
          error instanceof Error ? error.message : "unknown error",
        );
        void provider.stop(session.id);
      });
  }

  private event(id: string, runId: string, event: HarnessEvent): void {
    const live = this.live.get(id);
    if (
      !live ||
      live.value.runId !== runId ||
      live.value.status !== "running"
    )
      return;
    const session = applyHarnessEvent(live.value.session, event);
    if (session === live.value.session) return;
    live.value = { ...live.value, session };
    live.events.push(event);
    if (!BATCHED.has(event.type)) this.flush(id);
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
  ): HostSession {
    const session = {
      ...value.session,
      busy: false,
      pendingQuestion: undefined,
      backgroundTasks: undefined,
      // Rewrite only blocks that change, so the rest keep their sync stamps.
      blocks: value.session.blocks.map((block) =>
        block.streaming || (block.approval && !block.approval.decided)
          ? {
              ...block,
              streaming: false,
              ...(block.approval && !block.approval.decided
                ? {
                    approval: {
                      ...block.approval,
                      decided: "cancelled" as const,
                    },
                  }
                : {}),
            }
          : block,
      ),
    };
    if (message)
      session.blocks.push({
        id: randomUUID(),
        role: "system",
        text: message,
        streaming: false,
      });
    return { ...value, status, session };
  }

  async close(): Promise<void> {
    this.closing = true;
    await Promise.all(
      [...this.running.keys()].map((id) =>
        this.provider(this.store.session(id).session.harness).stop(id),
      ),
    );
    await Promise.all([...this.running.values()].map((active) => active.done));
  }
}
