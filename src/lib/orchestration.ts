import { invoke } from "@tauri-apps/api/core";
import { HARNESSES, type HarnessId, type Session } from "./session";
import type { ApprovalDecision, HarnessEvent } from "./harness/types";
import { pendingApprovalForSession } from "./approvalToast";
import type { UserQuestionReply } from "./userQuestion";
import {
  validateOrchestrationSettings,
  validateProposedTasks,
  type OrchestrationChoice,
  type OrchestrationProposal,
} from "./orchestrationPlan";

export type TaskStatus =
  "queued" | "running" | "cancelling" | "completed" | "failed" | "cancelled";
export type OrchestrationTask = {
  id: string;
  assignmentId?: string;
  sessionId: string;
  title: string;
  harness: HarnessId;
  model: string;
  modelSettings?: Record<string, string>;
  prompt: string;
  files: string[];
  scopes: string[];
  dependsOn: string[];
  status: TaskStatus;
  accepted: boolean;
  result: string;
  error?: string;
  delivered: boolean;
};
export type OrchestrationRun = {
  version: 1;
  leadId: string;
  cwd: string;
  canonicalRoot?: string;
  status: "active" | "paused" | "stopped" | "finished";
  allowedHarnesses: HarnessId[];
  allowedModels?: OrchestrationChoice[];
  proposalId?: string;
  maxWorkers: number;
  cli: string;
  tasks: OrchestrationTask[];
  error?: string;
  continuations: number;
  requests: Record<string, { signature: string; result: unknown }>;
};
export type ControlOutcome = {
  status: "completed" | "failed" | "cancelled";
  text: string;
  error?: string;
};
export type OrchestrationHost = {
  session(id: string): Session | undefined;
  sessions(): Session[];
  choices(): { harness: HarnessId; models: { id: string; name: string }[] }[];
  createWorker(run: OrchestrationRun, task: OrchestrationTask): Promise<void>;
  submit(
    id: string,
    text: string,
    done: (outcome: ControlOutcome) => void,
  ): void;
  stop(id: string): Promise<void>;
  /** Redirect a worker mid-turn, without discarding what it has already done. */
  steer(id: string, text: string): Promise<void>;
  /** Answer on a worker's behalf; the lead, not the user, decides. */
  respondApproval(
    id: string,
    requestId: number,
    decision: ApprovalDecision,
  ): void;
  answerQuestion(id: string, requestId: number, reply: UserQuestionReply): void;
};
type Storage = {
  save(run: OrchestrationRun): Promise<void>;
  load(id: string): Promise<OrchestrationRun | null>;
  enable(id: string, cwd: string): Promise<string>;
  disable(id: string): Promise<void>;
  scopes(cwd: string, files: string[]): Promise<string[]>;
};
const storage: Storage = {
  save: (run) =>
    invoke("control_save", { leadId: run.leadId, state: JSON.stringify(run) }),
  load: async (id) => {
    const raw = await invoke<string | null>("control_load", { leadId: id });
    if (!raw) return null;
    const run = JSON.parse(raw) as OrchestrationRun;
    if (run.version !== 1 || run.leadId !== id || !Array.isArray(run.tasks))
      throw new Error("Unsupported orchestration history");
    return run;
  },
  enable: (sessionId, cwd) => invoke("control_enable", { sessionId, cwd }),
  disable: (sessionId) => invoke("control_disable", { sessionId }),
  scopes: (cwd, files) => invoke("control_scopes", { cwd, files }),
};

export function scopesOverlap(a: string[], b: string[]): boolean {
  return a.some((left) =>
    b.some(
      (right) =>
        left === right ||
        left.startsWith(`${right}/`) ||
        right.startsWith(`${left}/`),
    ),
  );
}
const activeTask = (task: OrchestrationTask) =>
  task.status === "running" || task.status === "cancelling";
export const sameCheckout = (a: string, b: string) =>
  a.replace(/\\/g, "/").replace(/\/$/, "").toLowerCase() ===
  b.replace(/\\/g, "/").replace(/\/$/, "").toLowerCase();
const messageOf = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

const ASSIGNMENT_BLOCK =
  /(?:\r?\n[ \t]*)*<monocode_assignment\b[^>]*>[\s\S]*?<\/monocode_assignment>/gi;

/** Prompt the worker receives, including the envelope the transcript hides. */
export function workerTurnPrompt(prompt: string, files: string[]): string {
  return `${prompt}\n\n<monocode_assignment>\nYou are a worker managed by a MonoCode lead. Work in this shared checkout. Your assigned write scope is: ${files.join(", ")}. Read other files as needed, but do not edit outside your scope. If another file or shared operation is needed, report the blocker and stop so the lead can assign a new task. Do not spawn agents, create worktrees, switch branches, stage/commit changes, install dependencies or run broad formatters/generators unless this task owns '.' and explicitly requires that operation. Do not undo another agent's changes. Other workers may be editing concurrently; report focused checks, changed files, remaining issues and a concise final result.\n</monocode_assignment>`;
}

/** Task text a person should see: the assignment envelope stays in the send. */
export function visibleUserPrompt(text: string): string {
  return text.replace(ASSIGNMENT_BLOCK, "").trimEnd();
}

function text(value: unknown, label: string, max = 30_000): string {
  if (typeof value !== "string" || !value.trim())
    throw new Error(`Invalid ${label}: provide a non-empty string`);
  if (value.length > max)
    throw new Error(`Invalid ${label}: keep it under ${max} characters`);
  return value.trim();
}
function strings(value: unknown, label: string, max = 64): string[] {
  if (!Array.isArray(value))
    throw new Error(`Invalid ${label}: provide an array of strings`);
  if (value.length > max)
    throw new Error(`Invalid ${label}: at most ${max} entries`);
  return [...new Set(value.map((item) => text(item, label, 512)))];
}
/**
 * A mistyped field must fail loudly rather than silently change the task. A
 * Map, so an action named after an Object member is still just unknown.
 */
const FIELDS = new Map<string, string[]>([
  ["list", []],
  ["delegate", ["title", "harness", "model", "prompt", "files", "dependsOn"]],
  ["get", ["taskId"]],
  ["message", ["taskId", "text"]],
  ["cancel", ["taskId"]],
  ["wait", ["timeoutSeconds"]],
  ["review", ["taskId"]],
  ["finish", []],
  ["steer", ["taskId", "text"]],
  ["respond", ["taskId", "requestId", "decision"]],
  ["answer", ["taskId", "requestId", "answers", "skip"]],
]);
function checkFields(action: string, input: Record<string, unknown>) {
  const allowed = FIELDS.get(action);
  if (!allowed)
    throw new Error(
      `Unknown action "${action}". Use one of: ${[...FIELDS.keys()].join(", ")}. Run the control CLI with --help.`,
    );
  const unknown = Object.keys(input).filter((key) => !allowed.includes(key));
  if (unknown.length)
    throw new Error(
      `Unknown ${action} field${unknown.length > 1 ? "s" : ""}: ${unknown.join(", ")}. ${allowed.length ? `${action} accepts: ${allowed.join(", ")}.` : `${action} takes no input.`}`,
    );
}
/**
 * Quote the executable for the shell the lead runs in, and only when needed.
 * The path is absolute, so a leading slash means a POSIX shell — where a
 * backslash escapes rather than separates, and so is never safe bare.
 */
export function shellPath(path: string): string {
  if (path.startsWith("/"))
    return /^[A-Za-z0-9._/:-]+$/.test(path)
      ? path
      : `'${path.replace(/'/g, `'\\''`)}'`;
  return /[\s"]/.test(path) ? `"${path.replace(/"/g, "")}"` : path;
}
/** Map the lead's chosen option IDs onto the worker's own question shape. */
function questionAnswers(
  value: unknown,
  questions: { id: string; options: { id: string }[] }[] = [],
): Record<string, string[]> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(
      'answers must be an object of questionId -> [optionId], or pass {"skip":true}',
    );
  const answers: Record<string, string[]> = {};
  for (const [id, chosen] of Object.entries(value)) {
    const question = questions.find((entry) => entry.id === id);
    if (!question)
      throw new Error(
        `Unknown question "${id}". Ask for: ${listed(questions.map((entry) => entry.id)) || "none"}.`,
      );
    const ids = strings(chosen, `answers.${id}`, 16);
    const unknown = ids.filter(
      (option) => !question.options.some((entry) => entry.id === option),
    );
    if (unknown.length)
      throw new Error(
        `Unknown option${unknown.length > 1 ? "s" : ""} for "${id}": ${listed(unknown)}. Choose from: ${listed(question.options.map((entry) => entry.id))}.`,
      );
    answers[id] = ids;
  }
  if (!Object.keys(answers).length)
    throw new Error('Answer at least one question, or pass {"skip":true}');
  return answers;
}
const listed = (values: string[], max = 12) =>
  values.length > max
    ? `${values.slice(0, max).join(", ")} (+${values.length - max} more)`
    : values.join(", ");

export class Orchestrator {
  private runs: OrchestrationRun[] = [];
  private listeners = new Set<() => void>();
  private loaded = new Set<string>();
  private deleted = new Set<string>();
  private persisted = new Map<string, OrchestrationRun>();
  private saves = Promise.resolve();
  private actions = Promise.resolve();
  private pumping = false;
  private starting = new Set<string>();
  private pumpAgain = false;
  private waking = new Set<string>();
  private blocked = new Map<string, string>();
  private announced = new Map<string, Set<string>>();
  private inflight = new Map<
    string,
    { signature: string; promise: Promise<unknown> }
  >();
  private host: OrchestrationHost | null = null;
  constructor(private readonly store: Storage = storage) {}
  bind(host: OrchestrationHost) {
    this.host = host;
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  snapshot = () => this.runs;
  run(id: string) {
    return this.runs.find((run) => run.leadId === id);
  }
  forSession(id: string) {
    return this.runs.find(
      (run) =>
        run.leadId === id || run.tasks.some((task) => task.sessionId === id),
    );
  }
  private emit() {
    for (const listener of this.listeners) listener();
  }
  private async commit(run: OrchestrationRun) {
    this.runs = [
      ...this.runs.filter((entry) => entry.leadId !== run.leadId),
      run,
    ];
    this.emit();
    const saved = this.saves
      .catch(() => undefined)
      .then(async () => {
        await this.store.save(run);
        this.persisted.set(run.leadId, run);
      });
    this.saves = saved;
    try {
      await saved;
    } catch (error) {
      const current = this.run(run.leadId)!;
      const running = current.tasks.filter(activeTask);
      this.runs = this.runs.map((entry) =>
        entry.leadId === run.leadId
          ? {
              ...entry,
              status: "paused",
              error: `Could not save run: ${messageOf(error)}`,
              tasks: entry.tasks.map((task) =>
                activeTask(task) ? { ...task, status: "cancelling" } : task,
              ),
            }
          : entry,
      );
      this.emit();
      // Keep the checkout reserved until processes have stopped, even if the
      // database is unavailable. Recovery never repeats an interrupted turn.
      await Promise.allSettled(
        [run.leadId, ...running.map((task) => task.sessionId)].map(
          async (id) => {
            await this.host?.stop(id);
            const latest = this.run(run.leadId)!;
            this.runs = this.runs.map((entry) =>
              entry.leadId === run.leadId
                ? {
                    ...latest,
                    tasks: latest.tasks.map((task) =>
                      task.sessionId === id && activeTask(task)
                        ? {
                            ...task,
                            status: "failed",
                            accepted: false,
                            delivered: true,
                            error:
                              "Stopped because run history could not be saved. Review files before retrying.",
                          }
                        : task,
                    ),
                  }
                : entry,
            );
            this.emit();
          },
        ),
      );
      throw error;
    }
  }
  private async patchTask(
    leadId: string,
    id: string,
    patch: Partial<OrchestrationTask>,
  ) {
    const run = this.run(leadId);
    if (run)
      await this.commit({
        ...run,
        tasks: run.tasks.map((task) =>
          task.id === id ? { ...task, ...patch } : task,
        ),
      });
  }
  async hydrate(id: string) {
    if (this.loaded.has(id) || this.run(id)) return;
    this.loaded.add(id);
    try {
      const run = await this.store.load(id);
      if (!run || this.run(id) || this.deleted.has(id)) return;
      if (run.status === "active" || run.tasks.some(activeTask)) {
        await Promise.all(
          [
            id,
            ...run.tasks.filter(activeTask).map((task) => task.sessionId),
          ].map((sessionId) => this.host?.stop(sessionId)),
        );
        await this.store.disable(id);
      }
      await this.commit({
        ...run,
        status: run.status === "active" ? "paused" : run.status,
        tasks: run.tasks.map((task) =>
          activeTask(task)
            ? {
                ...task,
                status: "failed",
                accepted: false,
                error: "Interrupted. Review the shared files before retrying.",
                delivered: false,
              }
            : { ...task, delivered: task.accepted || task.status === "queued" },
        ),
        error:
          run.status === "active"
            ? "Run interrupted. Review the files, then resume."
            : run.error,
      });
    } catch (error) {
      this.loaded.delete(id);
      throw error;
    }
  }
  async start(
    leadId: string,
    allowedHarnesses: HarnessId[],
    maxWorkers: number,
    approved?: {
      proposalId: string;
      allowedModels: OrchestrationChoice[];
      tasks: OrchestrationTask[];
    },
  ) {
    if (this.starting.has(leadId))
      throw new Error("The run is already starting");
    this.starting.add(leadId);
    try {
      await this.startRun(leadId, allowedHarnesses, maxWorkers, approved);
    } finally {
      this.starting.delete(leadId);
    }
  }
  async startApproved(
    leadId: string,
    proposalId: string,
    proposal: OrchestrationProposal,
  ) {
    if (proposal.status !== "ready")
      throw new Error("Review a completed proposal before starting");
    const settings = validateOrchestrationSettings(proposal.settings);
    const planned = validateProposedTasks(
      proposal.tasks,
      settings,
      proposal.cwd,
    );
    const lead = this.host?.session(leadId);
    if (!lead || !sameCheckout(lead.cwd, proposal.cwd))
      throw new Error("Return to the proposal's project before starting");
    const available = this.host!.choices();
    const allowedModels = settings.choices.filter((choice) =>
      available.some(
        (entry) =>
          entry.harness === choice.harness &&
          entry.models.some((model) => model.id === choice.model),
      ),
    );
    if (
      planned.some(
        (task) =>
          !allowedModels.some(
            (choice) =>
              choice.harness === task.harness && choice.model === task.model,
          ),
      )
    )
      throw new Error(
        "An assigned model is no longer available. Change that assignment before starting.",
      );
    const ids = new Map(planned.map((task) => [task.id, crypto.randomUUID()]));
    const tasks: OrchestrationTask[] = await Promise.all(
      planned.map(async (task) => ({
        ...task,
        id: ids.get(task.id)!,
        assignmentId: task.id,
        sessionId: crypto.randomUUID(),
        scopes: await this.store.scopes(lead.cwd, task.files),
        dependsOn: task.dependsOn.map((id) => ids.get(id)!),
        status: "queued",
        accepted: false,
        delivered: true,
        result: "",
      })),
    );
    await this.start(
      leadId,
      [...new Set(allowedModels.map((choice) => choice.harness))],
      settings.maxWorkers,
      { proposalId, allowedModels, tasks },
    );
    this.host!.submit(
      leadId,
      `The user confirmed the orchestration card, including any edits. The app has already queued the exact assignments below; do not delegate duplicates. Supervise them through the control CLI, review their changes, request corrections when needed, and finish the original request.\n\nOriginal request:\n${proposal.request}\n\nApproved assignments:\n${JSON.stringify(tasks.map(({ id, title, prompt, harness, model, modelSettings, files, dependsOn }) => ({ taskId: id, title, prompt, harness, model, modelSettings, files, dependsOn })))}`,
      (outcome) => {
        if (outcome.status !== "completed")
          void this.pause(
            leadId,
            outcome.error ??
              "The lead was interrupted. Its agents were stopped; review and resume the run.",
          ).catch(console.error);
      },
    );
  }
  private async startRun(
    leadId: string,
    allowedHarnesses: HarnessId[],
    maxWorkers: number,
    approved?: {
      proposalId: string;
      allowedModels: OrchestrationChoice[];
      tasks: OrchestrationTask[];
    },
  ) {
    const lead = this.host?.session(leadId);
    if (!lead || lead.busy)
      throw new Error("Wait for the lead's current turn to finish");
    if (lead.worktreeCwd)
      throw new Error(
        "Start orchestration in a regular project session; this session uses a worktree",
      );
    if (!Number.isInteger(maxWorkers) || maxWorkers < 1 || maxWorkers > 4)
      throw new Error("Choose 1 to 4 workers");
    const available = this.host!.choices().map((choice) => choice.harness);
    if (
      !allowedHarnesses.length ||
      allowedHarnesses.some((id) => !available.includes(id))
    )
      throw new Error("Choose installed worker harnesses");
    if (
      this.host!.sessions().some(
        (session) =>
          session.id !== leadId &&
          session.busy &&
          sameCheckout(session.worktreeCwd ?? session.cwd, lead.cwd),
      )
    )
      throw new Error("Stop other running sessions in this checkout first");
    const previous = this.run(leadId);
    if (
      previous?.status === "active" ||
      (approved && previous?.status === "paused")
    )
      throw new Error("Stop the current run before starting another proposal");
    if (previous?.tasks.some(activeTask))
      throw new Error("Stop active workers before changing the run");
    const [canonicalRoot] = await this.store.scopes(lead.cwd, ["."]);
    const cli = await this.store.enable(leadId, lead.cwd);
    try {
      await this.host!.stop(leadId); // Refresh the child environment before its next turn.
      await this.commit({
        version: 1,
        leadId,
        cwd: lead.cwd,
        canonicalRoot,
        cli,
        status: "active",
        allowedHarnesses: [...new Set(allowedHarnesses)],
        allowedModels:
          approved?.allowedModels ??
          (previous?.status === "paused" ? previous.allowedModels : undefined),
        proposalId:
          approved?.proposalId ??
          (previous?.status === "paused" ? previous.proposalId : undefined),
        maxWorkers,
        tasks:
          approved?.tasks ??
          (previous?.status === "paused" ? previous.tasks : []),
        continuations: 0,
        requests: previous?.status === "paused" ? previous.requests : {},
      });
    } catch (error) {
      await this.store.disable(leadId);
      throw error;
    }
    void this.pump();
    this.sync();
  }
  submissionError(id: string, managed = false): string | null {
    if (managed) return null;
    const session = this.host?.session(id);
    if (!session) return null;
    const own = this.forSession(id);
    if (
      own &&
      own.leadId !== id &&
      (own.status === "active" || own.tasks.some(activeTask))
    )
      return "This worker is managed by the orchestrator. Send instructions through its lead or stop the run first.";
    const other = this.runs.find(
      (run) =>
        (run.status === "active" || run.tasks.some(activeTask)) &&
        run.leadId !== id &&
        sameCheckout(run.cwd, session.worktreeCwd ?? session.cwd),
    );
    if (other)
      return "This checkout has an active orchestrator. Stop that run before starting independent work.";
    if (own?.status === "paused")
      return "Resume or stop orchestration before sending the lead another turn.";
    if (
      own?.status === "active" &&
      !sameCheckout(own.cwd, session.worktreeCwd ?? session.cwd)
    )
      return "Return the lead to its original project or stop orchestration first.";
    return null;
  }
  prompt(id: string, prompt: string): string {
    const run = this.run(id);
    if (!run || run.status !== "active") return prompt;
    const cli = `${shellPath(run.cli)} control`;
    return `${prompt}\n\n<monocode_orchestration>\nYou are the lead of a local MonoCode run. Coordinate the user's task using ${cli}. Run \`${cli} --help\` before your first command; it documents every action, its exact JSON fields and the retry rule. Credentials are already in your environment; never print them.\nEach call prints one JSON line and exits non-zero unless "ok" is true; read the "error" text, it says what to do next. Unknown JSON fields are rejected rather than ignored, so fix the field name instead of guessing. If a call fails before reaching MonoCode, retry it with the "requestId" from that response so the work is never queued twice.\nUse list to discover allowed harness/model IDs. Delegate bounded tasks with project-relative files (directories reserve their descendants), self-contained prompts and dependsOn task IDs. Use the same checkout. You may read and plan; leave file edits to workers. Never start workers outside this CLI. Workers with overlapping files are queued. For installs, Git mutations, generators or broad formatting, assign a separate task with files ["."] and wait for other workers to finish.\nAgents never prompt the user. When one needs an approval or answers a question, list, get and wait report it as needsInput on that task, and you decide with respond or answer; it stays stopped until you do. Judge the request against the task you assigned, and put it to the user in this conversation only when the call is genuinely theirs.\nSteer a running agent with steer to correct its course without losing its work; use message only once it has stopped. Read results with get or wait; completed means a turn finished, not that the work passed review. Review the actual changes, message a worker for fixes, and use review to accept each completed task. Cancel discarded tasks. A failed task blocks finish until you retry it with message or drop it with cancel. Call finish only when required work and combined validation are complete. You receive worker results automatically when idle; use bounded wait calls while supervising. Do not expose credentials, use worktrees, switch branches or silently escalate worker permissions.\n</monocode_orchestration>`;
  }
  async handle(
    leadId: string,
    requestId: string,
    action: string,
    input: Record<string, unknown>,
  ): Promise<unknown> {
    checkFields(action, input);
    const signature = JSON.stringify({ action, input });
    const key = `${leadId}:${requestId}`;
    const receipts = this.persisted.get(leadId)?.requests;
    const previous =
      receipts &&
      Object.prototype.hasOwnProperty.call(receipts, requestId) &&
      typeof receipts[requestId]?.signature === "string"
        ? receipts[requestId]
        : undefined;
    if (previous) {
      if (previous.signature !== signature)
        throw new Error("Request ID was already used with different input");
      return previous.result;
    }
    const pending = this.inflight.get(key);
    if (pending) {
      if (pending.signature !== signature)
        throw new Error("Request ID was already used with different input");
      return pending.promise;
    }
    if (action === "wait") return this.wait(leadId, input);
    const result = this.actions
      .catch(() => undefined)
      .then(async () => {
        const run = this.run(leadId);
        if (!run || run.status !== "active")
          throw new Error("This run is not active");
        if (
          !["list", "get"].includes(action) &&
          Object.keys(run.requests).length >= 512
        )
          throw new Error(
            "Run command limit reached. Stop the run and start another after reviewing the files.",
          );
        return this.execute(run, action, input, requestId, signature);
      });
    this.actions = result.then(
      () => undefined,
      () => undefined,
    );
    this.inflight.set(key, { signature, promise: result });
    try {
      return await result;
    } finally {
      this.inflight.delete(key);
      void this.pump();
    }
  }
  private view(run: OrchestrationRun) {
    return {
      ...run,
      cli: undefined,
      requests: undefined,
      tasks: run.tasks.map((task) => ({
        ...task,
        prompt: undefined,
        scopes: undefined,
        waitingFor: this.waitingFor(run, task),
        needsInput: this.pendingInput(task),
      })),
    };
  }
  /**
   * What a worker is blocked on. Workers have no user-facing prompt: the lead
   * answers for them, and escalates to the user in its own conversation when
   * it does not want to decide alone.
   */
  pendingInput(task: OrchestrationTask) {
    const worker = this.host?.session(task.sessionId);
    const pending = worker && pendingApprovalForSession(worker);
    if (!pending) return undefined;
    return {
      kind: pending.kind,
      requestId: pending.requestId,
      label: pending.label,
      detail:
        pending.kind === "approval"
          ? (pending.block?.tool?.detail?.trim() ?? pending.block?.text)
          : undefined,
      questions: worker?.pendingQuestion?.questions,
    };
  }
  waitingFor(
    run: OrchestrationRun,
    task: OrchestrationTask,
  ): string | undefined {
    if (task.status !== "queued") return undefined;
    const dependency = run.tasks.find(
      (entry) => task.dependsOn.includes(entry.id) && !entry.accepted,
    );
    if (dependency) return `Waiting for review: ${dependency.title}`;
    const owner = run.tasks.find(
      (entry) => activeTask(entry) && scopesOverlap(entry.scopes, task.scopes),
    );
    if (owner) return `Waiting for files: ${owner.title}`;
    return "Waiting for a worker slot";
  }
  private async execute(
    run: OrchestrationRun,
    action: string,
    input: Record<string, unknown>,
    requestId: string,
    signature: string,
  ): Promise<unknown> {
    // Persist the mutation and its retry receipt together, before dispatch.
    const record = async (next: OrchestrationRun, result: unknown) => {
      await this.commit({
        ...next,
        requests: { ...next.requests, [requestId]: { signature, result } },
      });
      return result;
    };
    const changeTask = (
      id: string,
      patch: Partial<OrchestrationTask>,
      result: unknown,
    ) => {
      const current = this.run(run.leadId)!;
      return record(
        {
          ...current,
          tasks: current.tasks.map((entry) =>
            entry.id === id ? { ...entry, ...patch } : entry,
          ),
        },
        result,
      );
    };
    const task = () => {
      const id = text(input.taskId, "taskId", 128);
      const found = this.run(run.leadId)!.tasks.find(
        (entry) => entry.id === id,
      );
      if (!found)
        throw new Error(
          "Task does not belong to this run. Use a taskId returned by delegate or list.",
        );
      return found;
    };
    switch (action) {
      case "list":
        return {
          run: this.view(run),
          harnesses: this.host!.choices()
            .filter((choice) => run.allowedHarnesses.includes(choice.harness))
            .map((choice) => ({
              ...choice,
              models: choice.models.filter(
                (model) =>
                  !run.allowedModels ||
                  run.allowedModels.some(
                    (allowed) =>
                      allowed.harness === choice.harness &&
                      allowed.model === model.id,
                  ),
              ),
            })),
        };
      case "get": {
        // The fields list reports, plus this task's own prompt.
        const target = task();
        return {
          ...target,
          scopes: undefined,
          waitingFor: this.waitingFor(this.run(run.leadId)!, target),
          needsInput: this.pendingInput(target),
        };
      }
      case "delegate": {
        if (run.tasks.length >= 40)
          throw new Error("This run has reached its 40-task limit");
        const harness = text(input.harness, "harness") as HarnessId;
        if (
          !HARNESSES.includes(harness) ||
          !run.allowedHarnesses.includes(harness)
        )
          throw new Error(
            `Harness "${harness}" is not allowed in this run. Allowed: ${listed(run.allowedHarnesses)}.`,
          );
        const choice = this.host!.choices().find(
          (entry) => entry.harness === harness,
        );
        if (!choice) throw new Error("Worker harness is unavailable");
        const permittedModels = choice.models.filter(
          (model) =>
            !run.allowedModels ||
            run.allowedModels.some(
              (allowed) =>
                allowed.harness === harness && allowed.model === model.id,
            ),
        );
        const model =
          input.model == null
            ? permittedModels[0]?.id
            : text(input.model, "model", 256);
        if (!model || !permittedModels.some((item) => item.id === model))
          throw new Error(
            `Choose a model ID returned by list for ${harness}: ${listed(permittedModels.map((item) => item.id)) || "none available"}.`,
          );
        const title = text(input.title, "title", 160);
        const prompt = text(input.prompt, "prompt");
        const files = strings(input.files, "files");
        if (!files.length)
          throw new Error(
            "Declare at least one file/directory scope in files, or '.' for exclusive checkout access",
          );
        const dependsOn = strings(input.dependsOn ?? [], "dependsOn", 40);
        const missing = dependsOn.filter(
          (id) =>
            !run.tasks.some(
              (item) => item.id === id && item.status !== "cancelled",
            ),
        );
        if (missing.length)
          throw new Error(
            `dependsOn must hold taskIds from this run; unknown or cancelled: ${listed(missing)}.`,
          );
        const scopes = await this.store.scopes(run.cwd, files);
        const created: OrchestrationTask = {
          id: crypto.randomUUID(),
          sessionId: crypto.randomUUID(),
          title,
          prompt,
          files,
          scopes,
          dependsOn,
          harness,
          model,
          status: "queued",
          accepted: false,
          result: "",
          delivered: true,
        };
        return record(
          {
            ...this.run(run.leadId)!,
            tasks: [...this.run(run.leadId)!.tasks, created],
          },
          {
            taskId: created.id,
            sessionId: created.sessionId,
            status: "queued",
          },
        );
      }
      case "message": {
        const target = task();
        if (activeTask(target) || target.status === "queued")
          throw new Error(
            `Wait for this worker or cancel it before sending a new turn; ${target.title} is ${target.status}.`,
          );
        if (
          run.tasks.some(
            (entry) =>
              entry.dependsOn.includes(target.id) &&
              entry.status !== "queued" &&
              entry.status !== "cancelled",
          )
        )
          throw new Error(
            "A dependent task has already started; create a separate correction task after it finishes",
          );
        return changeTask(
          target.id,
          {
            prompt: text(input.text, "text"),
            status: "queued",
            accepted: false,
            result: "",
            error: undefined,
            delivered: true,
          },
          { taskId: target.id, status: "queued" },
        );
      }
      case "steer": {
        const target = task();
        if (target.status !== "running")
          throw new Error(
            `Only a running agent can be steered; ${target.title} is ${target.status}. ${
              target.status === "queued"
                ? "It has not started yet, so edit it with message instead."
                : "Send it a fresh turn with message."
            }`,
          );
        const guidance = text(input.text, "text");
        await this.host!.steer(target.sessionId, guidance);
        return record(this.run(run.leadId)!, {
          taskId: target.id,
          steered: true,
        });
      }
      case "cancel":
        await this.cancelTask(run.leadId, task().id);
        return record(this.run(run.leadId)!, { cancelled: true });
      case "respond": {
        const target = task();
        const pending = this.pendingInput(target);
        if (pending?.kind !== "approval")
          throw new Error(
            `${target.title} is not waiting on an approval. Read needsInput from list or wait before responding.`,
          );
        if (input.requestId !== pending.requestId)
          throw new Error(
            `Stale requestId. ${target.title} is waiting on ${pending.requestId}.`,
          );
        const decision = text(input.decision, "decision", 16);
        if (decision !== "allow" && decision !== "deny")
          throw new Error('decision must be "allow" or "deny"');
        this.host!.respondApproval(target.sessionId, pending.requestId, decision);
        return record(this.run(run.leadId)!, {
          taskId: target.id,
          decision,
        });
      }
      case "answer": {
        const target = task();
        const pending = this.pendingInput(target);
        if (pending?.kind !== "question")
          throw new Error(
            `${target.title} is not waiting on a question. Read needsInput from list or wait before answering.`,
          );
        if (input.requestId !== pending.requestId)
          throw new Error(
            `Stale requestId. ${target.title} is waiting on ${pending.requestId}.`,
          );
        const reply: UserQuestionReply =
          input.skip === true
            ? { kind: "skipped" }
            : {
                kind: "answered",
                answers: questionAnswers(input.answers, pending.questions),
              };
        this.host!.answerQuestion(target.sessionId, pending.requestId, reply);
        return record(this.run(run.leadId)!, {
          taskId: target.id,
          answered: reply.kind === "answered",
        });
      }
      case "review": {
        const target = task();
        if (target.status !== "completed")
          throw new Error(
            `Only a completed result can be accepted; ${target.title} is ${target.status}. ${
              target.status === "failed"
                ? "Send it another turn with message, or drop it with cancel."
                : "Wait for it to finish, or cancel it."
            }`,
          );
        return changeTask(target.id, { accepted: true }, { accepted: true });
      }
      case "finish": {
        const outstanding = this.run(run.leadId)!.tasks.filter(
          (entry) => entry.status !== "cancelled" && !entry.accepted,
        );
        if (outstanding.length)
          throw new Error(
            `Review all remaining tasks before finishing. Outstanding: ${listed(
              outstanding.map((entry) => `${entry.title} (${entry.status})`),
            )}. Accept a completed task with review, send a failed one another turn with message, or drop it with cancel.`,
          );
        const result = await record(
          { ...this.run(run.leadId)!, status: "finished" },
          { finished: true },
        );
        await this.store.disable(run.leadId);
        return result;
      }
      default:
        throw new Error("Unknown action. Run control --help.");
    }
  }
  private async wait(leadId: string, input: Record<string, unknown>) {
    const run = this.run(leadId);
    if (!run || run.status !== "active")
      throw new Error("This run is not active");
    const seconds = input.timeoutSeconds ?? 20;
    if (
      typeof seconds !== "number" ||
      !Number.isFinite(seconds) ||
      seconds < 0 ||
      seconds > 25
    )
      throw new Error("timeoutSeconds must be 0 to 25");
    // A worker blocking on the lead changes no run state, so watch for that
    // separately; otherwise the lead sleeps while an agent waits on it.
    const blocked = this.blocked.get(leadId);
    if (
      run.tasks.some(activeTask) ||
      run.tasks.some((task) => task.status === "queued")
    ) {
      await new Promise<void>((resolve) => {
        const finish = () => {
          clearTimeout(timer);
          unsubscribe();
          resolve();
        };
        const unsubscribe = this.subscribe(() => {
          if (this.run(leadId) !== run || this.blocked.get(leadId) !== blocked)
            finish();
        });
        const timer = setTimeout(finish, seconds * 1000);
      });
    }
    return this.view(this.run(leadId)!);
  }
  private async pump() {
    if (this.pumping) {
      this.pumpAgain = true;
      return;
    }
    if (!this.host) return;
    this.pumping = true;
    try {
      for (const initial of this.runs) {
        const initialRun = this.run(initial.leadId);
        if (!initialRun || initialRun.status !== "active") continue;
        for (const initialTask of initialRun.tasks) {
          const run = this.run(initial.leadId);
          if (
            !run ||
            run.status !== "active" ||
            run.tasks.filter(activeTask).length >= run.maxWorkers
          )
            break;
          const task = run.tasks.find((entry) => entry.id === initialTask.id);
          if (
            !task ||
            task.status !== "queued" ||
            task.dependsOn.some(
              (id) => !run.tasks.find((entry) => entry.id === id)?.accepted,
            )
          )
            continue;
          if (
            run.tasks.some(
              (entry) =>
                activeTask(entry) && scopesOverlap(entry.scopes, task.scopes),
            )
          )
            continue;
          if (!this.host.session(run.leadId)) {
            await this.stopRun(run.leadId);
            break;
          }
          await this.patchTask(run.leadId, task.id, { status: "running" });
          try {
            if (
              !this.host
                .choices()
                .some(
                  (choice) =>
                    choice.harness === task.harness &&
                    choice.models.some((model) => model.id === task.model),
                )
            )
              throw new Error(
                "The assigned harness/model is no longer available. Review this task before retrying.",
              );
            await this.host.createWorker(run, task);
            if (
              this.run(run.leadId)?.status !== "active" ||
              this.run(run.leadId)?.tasks.find((entry) => entry.id === task.id)
                ?.status !== "running"
            )
              continue;
            const prompt = workerTurnPrompt(task.prompt, task.files);
            this.host.submit(task.sessionId, prompt, (outcome) => {
              void this.settle(run.leadId, task.id, outcome).catch(
                console.error,
              );
            });
          } catch (error) {
            await this.settle(run.leadId, task.id, {
              status: "failed",
              text: "",
              error: messageOf(error),
            });
          }
        }
      }
    } catch (error) {
      console.error("Orchestration dispatch failed", error);
    } finally {
      this.pumping = false;
      if (this.pumpAgain) {
        this.pumpAgain = false;
        void this.pump();
      }
    }
  }
  private async settle(
    leadId: string,
    taskId: string,
    outcome: ControlOutcome,
  ) {
    const task = this.run(leadId)?.tasks.find((entry) => entry.id === taskId);
    if (!task || task.status !== "running") return;
    await this.patchTask(leadId, taskId, {
      status: outcome.status,
      result: outcome.text.slice(-20_000),
      error: outcome.error,
      delivered: false,
      accepted: false,
    });
    void this.pump();
    this.sync();
  }
  async cancelTask(leadId: string, taskId: string) {
    const task = this.run(leadId)?.tasks.find((entry) => entry.id === taskId);
    if (!task || task.status === "cancelled") return;
    if (activeTask(task)) {
      await this.patchTask(leadId, taskId, { status: "cancelling" });
      await this.host!.stop(task.sessionId);
    }
    await this.patchTask(leadId, taskId, {
      status: "cancelled",
      accepted: false,
      delivered: false,
    });
    void this.pump();
  }
  /**
   * A paused run has no supervisor, so its agents stop with it. Leaving them
   * editing the shared checkout with nobody reviewing is how a run quietly
   * diverges from what the user approved. Queued work is left alone: `pump`
   * will not dispatch while paused, so it resumes intact.
   */
  private async pause(
    leadId: string,
    error: string,
    patch?: (run: OrchestrationRun) => OrchestrationRun,
  ) {
    const run = this.run(leadId);
    if (!run || run.status !== "active") return;
    await this.commit({ ...(patch ? patch(run) : run), status: "paused", error });
    await Promise.all(
      this.run(leadId)!
        .tasks.filter(activeTask)
        .map((task) => this.cancelTask(leadId, task.id)),
    );
  }
  async stopRun(leadId: string) {
    const run = this.run(leadId);
    if (!run) return;
    let saveError: unknown;
    await this.commit({ ...run, status: "stopped" }).catch((error: unknown) => {
      saveError = error;
    });
    await Promise.all(
      [
        leadId,
        ...run.tasks.filter(activeTask).map((task) => task.sessionId),
      ].map((id) => this.host?.stop(id)),
    );
    const latest = this.run(leadId)!;
    const stopped: OrchestrationRun = {
      ...latest,
      status: "stopped",
      tasks: latest.tasks.map((task) =>
        activeTask(task) || task.status === "queued"
          ? { ...task, status: "cancelled", accepted: false, delivered: true }
          : task,
      ),
    };
    await this.commit(stopped).catch((error: unknown) => {
      saveError = error;
    });
    this.runs = this.runs.map((entry) =>
      entry.leadId === leadId
        ? {
            ...stopped,
            error: saveError
              ? `Run stopped; could not save history: ${messageOf(saveError)}`
              : undefined,
          }
        : entry,
    );
    this.emit();
    await this.store.disable(leadId);
  }
  stopForSession(id: string): Promise<void> | null {
    const run = this.forSession(id);
    if (
      !run ||
      (run.status !== "active" &&
        run.status !== "paused" &&
        !run.tasks.some(activeTask))
    )
      return null;
    if (run.leadId === id) return this.stopRun(id);
    const task = run.tasks.find((entry) => entry.sessionId === id)!;
    return this.cancelTask(run.leadId, task.id);
  }
  /** Drain control writes before the database removes a lead or one of its workers. */
  deleteSession(id: string, remove: () => Promise<void>): Promise<void> {
    const result = this.actions.catch(() => undefined).then(async () => {
      const run = this.forSession(id);
      if (
        run &&
        (run.status === "active" ||
          run.status === "paused" ||
          run.tasks.some(activeTask))
      ) {
        await this.stopRun(run.leadId);
      }
      await this.saves.catch(() => undefined);
      await remove();
      this.deleted.add(id);
      if (!run) return;
      this.runs = this.runs.filter((entry) => entry.leadId !== run.leadId);
      this.persisted.delete(run.leadId);
      this.blocked.delete(run.leadId);
      this.announced.delete(run.leadId);
      this.emit();
      if (run.leadId !== id) {
        // Read the transaction's pruned graph; never save the pre-delete snapshot.
        const updated = await this.store.load(run.leadId).catch((error) => {
          console.error("Could not reload orchestration after deletion", error);
          this.loaded.delete(run.leadId);
          return null;
        });
        if (updated) {
          this.runs = [...this.runs, updated];
          this.persisted.set(run.leadId, updated);
          this.emit();
        }
      }
    });
    this.actions = result.catch(() => undefined);
    return result;
  }
  /** `${taskId}:${requestId}` for every worker currently blocked on the lead. */
  private blockedKeys(run: OrchestrationRun): string[] {
    return run.tasks.flatMap((task) => {
      const pending = this.pendingInput(task);
      return pending ? [`${task.id}:${pending.requestId}`] : [];
    });
  }
  sync() {
    for (const run of this.runs) {
      if (run.status !== "active") continue;
      // A blocked worker changes no run state, so `wait` needs telling.
      const keys = this.blockedKeys(run).join(",");
      if (this.blocked.get(run.leadId) !== keys) {
        this.blocked.set(run.leadId, keys);
        this.emit();
      }
    }
    for (const run of this.runs) {
      if (run.status !== "active" || this.waking.has(run.leadId)) continue;
      const lead = this.host?.session(run.leadId);
      if (!lead || lead.busy || lead.queuedMessages?.length) continue;
      const announced = this.announced.get(run.leadId) ?? new Set<string>();
      const results = run.tasks.filter((task) => !task.delivered);
      const blocked = this.blockedKeys(run).filter(
        (key) => !announced.has(key),
      );
      if (!results.length && !blocked.length) continue;
      this.waking.add(run.leadId);
      // Let session state settle before checking idle; never interrupt user input.
      setTimeout(() => {
        void (async () => {
          try {
            const current = this.run(run.leadId);
            const session = this.host?.session(run.leadId);
            if (
              !current ||
              current.status !== "active" ||
              !session ||
              session.busy ||
              session.queuedMessages?.length
            )
              return;
            const results = current.tasks.filter(
              (task) =>
                !task.delivered &&
                !activeTask(task) &&
                task.status !== "queued",
            );
            const seen = this.announced.get(run.leadId) ?? new Set<string>();
            const waiting = current.tasks.flatMap((task) => {
              const pending = this.pendingInput(task);
              const key = pending && `${task.id}:${pending.requestId}`;
              return key && !seen.has(key) ? [{ task, pending, key }] : [];
            });
            if (!results.length && !waiting.length) return;
            if (current.continuations >= 20) {
              await this.pause(
                run.leadId,
                "Automatic continuation limit reached. Its agents were stopped; review and resume the run.",
              );
              return;
            }
            await this.commit({
              ...current,
              continuations: current.continuations + 1,
              tasks: current.tasks.map((task) =>
                results.some((item) => item.id === task.id)
                  ? { ...task, delivered: true }
                  : task,
              ),
            });
            this.announced.set(
              run.leadId,
              new Set([...seen, ...waiting.map((entry) => entry.key)]),
            );
            const summary = results
              .map(
                (task) =>
                  `${task.id} — ${task.title}: ${task.status}\n${task.error ?? ""}\n${task.result.slice(-4000)}`,
              )
              .join("\n\n");
            const asks = waiting
              .map(
                ({ task, pending }) =>
                  `${task.id} — ${task.title} needs ${pending.kind === "approval" ? "an approval" : "an answer"} (requestId ${pending.requestId}): ${pending.label}\n${pending.detail?.slice(0, 2000) ?? ""}${
                    pending.questions
                      ? `\n${JSON.stringify(pending.questions)}`
                      : ""
                  }`,
              )
              .join("\n\n");
            const body = [
              results.length
                ? `Worker results are ready. Review the work, request corrections through the CLI when needed, and finish the original task.\n\n${summary}`
                : "",
              waiting.length
                ? `These agents are blocked waiting on you. Decide each one with respond or answer; they stay stopped until you do. Judge it against the task you assigned, and ask the user in this conversation only when the call is genuinely theirs to make.\n\n${asks}`
                : "",
            ]
              .filter(Boolean)
              .join("\n\n");
            this.host!.submit(
              run.leadId,
              body,
              (outcome) => {
                if (outcome.status !== "completed") {
                  void this.pause(
                    run.leadId,
                    outcome.error ??
                      "Lead continuation was interrupted. Its agents were stopped; review and resume.",
                    (current) => ({
                      ...current,
                      tasks: current.tasks.map((task) =>
                        results.some((item) => item.id === task.id)
                          ? { ...task, delivered: false }
                          : task,
                      ),
                    }),
                  ).catch(console.error);
                } else this.sync();
              },
            );
          } catch (error) {
            console.error("Orchestration continuation failed", error);
          } finally {
            this.waking.delete(run.leadId);
          }
        })();
      }, 0);
    }
  }
  observe(id: string, event: HarnessEvent) {
    if (event.type !== "tool.started" && event.type !== "tool.updated") return;
    const run = this.forSession(id);
    const task = run?.tasks.find(
      (entry) => entry.sessionId === id && entry.status === "running",
    );
    if (
      !run ||
      run.status !== "active" ||
      !task ||
      event.preview?.kind !== "write"
    )
      return;
    const paths =
      event.paths ?? (event.preview.path ? [event.preview.path] : []);
    for (const path of paths) {
      const absolute = /^(\/|[a-z]:[\\/])/i.test(path)
        ? path
        : `${run.canonicalRoot ?? run.cwd}/${path}`;
      const parts: string[] = [];
      for (const part of absolute
        .replace(/\\/g, "/")
        .toLowerCase()
        .split("/")) {
        if (part === "..") parts.pop();
        else if (part !== ".") parts.push(part);
      }
      const normalized = parts.join("/");
      if (
        task.scopes.some(
          (scope) => normalized === scope || normalized.startsWith(`${scope}/`),
        )
      )
        continue;
      void (async () => {
        await this.commit({
          ...this.run(run.leadId)!,
          status: "paused",
          error: `${task.title} reported a write outside its assignment: ${path}. Review the shared files before resuming.`,
        });
        await Promise.all(
          this.run(run.leadId)!
            .tasks.filter(activeTask)
            .map((entry) => this.cancelTask(run.leadId, entry.id)),
        );
      })().catch(console.error);
      break;
    }
  }
}

export const orchestrator = new Orchestrator();
