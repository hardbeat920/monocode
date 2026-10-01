import { sanitizeAcceptedReference, type AssignmentReference } from "./assignments";
import { invoke } from "@tauri-apps/api/core";
import type { Session } from "../../sessions/model/session";
import type { ControlOutcome } from "../../orchestration/model/orchestration";
import { pendingApprovalForSession } from "../../notifications/model/approvalToast";
import { isPreparingHandoff } from "../../sessions/model/handoff";

export type SessionUpdateStatus =
  | "settled"
  | "failed"
  | "stopped"
  | "interrupted"
  | "removed"
  | "approval"
  | "question";

export type SessionUpdate = {
  /**
   * outcome: `${childId}:${generation}:outcome`, or `${childId}:${generation}:removed`
   * for an idle child's deletion; blocked: `${childId}:${kind}:${requestId}`
   */
  id: string;
  kind: "outcome" | "blocked";
  status: SessionUpdateStatus;
  /** The parent this update was recorded for; delivery groups by it. */
  parentId: string;
  childId: string;
  generation: number;
  assignment?: AssignmentReference;
  title: string;
  harness: string;
  model?: string;
  /** Last 4000 characters of the turn text. */
  excerpt?: string;
  truncated?: boolean;
  error?: string;
  requestId?: number;
  label?: string;
  /** An earlier delivery turn showing this update failed without output. */
  repeat?: boolean;
  at: number;
};

export type SessionLink = {
  version: 1;
  childId: string;
  parentId: string;
  generation: number;
  status: "running" | "idle";
  /** `app-${parentId}-${requestId}` of the last arming request. */
  lastRequestKey?: string;
  assignment?: AssignmentReference;
  lastOutcome?: { generation: number; status: SessionUpdateStatus };
  /** Recorded but held until the child's own children have reported to it. */
  heldOutcome?: SessionUpdate;
  /** Child deleted; the row goes once its `removed` update is delivered. */
  removing?: boolean;
  /** Terminal outcomes only; blocked updates live in memory. */
  pending: SessionUpdate[];
  inFlight?: { deliveryId: string; updates: SessionUpdate[] };
  /** Cross-window merge keeps the newer row. */
  updatedAt: number;
};

export type SessionLinkView = {
  childId: string;
  generation: number;
  status: "running" | "idle";
  held: boolean;
  pending: number;
  assignment?: AssignmentReference;
  lastOutcome?: SessionLink["lastOutcome"];
  removing?: boolean;
  title?: string;
  harness?: string;
  model?: string;
};

export type SessionLinkStore = {
  load(): Promise<SessionLink[]>;
  save(link: SessionLink): Promise<void>;
  remove(childId: string): Promise<void>;
};

export type SessionLinkHost = {
  /** Open in this window. */
  session(id: string): Session | undefined;
  /** From the database when not open. */
  stored(id: string): Promise<Session | undefined>;
  canAutoContinue(session: Session): boolean;
  submit(
    parentId: string,
    text: string,
    deliveryId: string,
    done: (outcome: ControlOutcome) => void,
    updates?: SessionUpdate[],
  ): void;
  /** Append a system error notice block. */
  notice(parentId: string, text: string): void;
  /** Returns cancel. */
  schedule(run: () => void, ms: number): () => void;
  now(): number;
};

/** The orchestrator injects the same tail of a worker's result. */
const EXCERPT_LIMIT = 4000;
/** Decision D7: five retries, about 31 s, covering the Pi settle gap. */
const RETRY_DELAYS_MS = [1000, 2000, 4000, 8000, 16000];

const OUTCOME_STATUS: Record<ControlOutcome["status"], SessionUpdateStatus> = {
  completed: "settled",
  failed: "failed",
  cancelled: "stopped",
};

const HEADER =
  "MonoCode session updates for sessions this thread started. Read a full reply with `sessions.read`; continue a session with `sessions.send`. A blocked session is waiting for the user, not for you.";

type Delivery = {
  deliveryId: string;
  parentId: string;
  /** Blocked updates riding this delivery; never persisted. */
  blocked: SessionUpdate[];
};

/** A parent that exhausted its retries waits for its next turn to end. */
type Stall = { userBlockId?: string; sawBusy: boolean };

function lastUserBlockId(session: Session | undefined): string | undefined {
  if (!session) return undefined;
  for (let index = session.blocks.length - 1; index >= 0; index -= 1) {
    if (session.blocks[index].role === "user") return session.blocks[index].id;
  }
  return undefined;
}

function lastAssistantTextInTurn(session: Session): string {
  for (let index = session.blocks.length - 1; index >= 0; index -= 1) {
    const block = session.blocks[index];
    if (block.role === "user") return "";
    if (block.role === "assistant" && block.text.trim()) return block.text;
  }
  return "";
}

function endsWithInterrupt(session: Session): boolean {
  const last = session.blocks[session.blocks.length - 1];
  return last?.role === "system" && last.notice === "interrupt";
}

export class SessionLinks {
  readonly ready: Promise<void>;
  private readonly store: SessionLinkStore;
  private host?: SessionLinkHost;
  private resolveReady: () => void = () => undefined;
  private links = new Map<string, SessionLink>();
  /** Undelivered blocked updates by id. */
  private blocked = new Map<string, SessionUpdate>();
  /** Blocked ids already delivered while their request is still pending. */
  private announced = new Set<string>();
  /** Deliveries this window submitted and is awaiting, by parent. */
  private deliveries = new Map<string, Delivery>();
  private issued = new Set<string>();
  private waking = new Set<string>();
  private retries = new Map<string, () => void>();
  private failures = new Map<string, number>();
  private stalls = new Map<string, Stall>();
  /**
   * Parents whose update turn was delivered while they hold an outcome. Their
   * own turn end follows and replaces it, so release must not run first.
   */
  private awaitingTurnEnd = new Set<string>();
  /** In-flight deliveries read at boot that this window may reconcile. */
  private bootInFlight = new Set<string>();
  private hydrating: Promise<void> = Promise.resolve();
  private reconciled = false;

  constructor(store: SessionLinkStore) {
    this.store = store;
    this.ready = new Promise((resolve) => {
      this.resolveReady = resolve;
    });
  }

  private listeners = new Set<() => void>();
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }
  private setLink(id: string, link: SessionLink) {
    this.links.set(id, link);
    for (const listener of this.listeners) listener();
  }
  private deleteLink(id: string) {
    this.links.delete(id);
    for (const listener of this.listeners) listener();
  }

  bind(host: SessionLinkHost) {
    this.host = host;
  }

  hydrate(options: { boot?: boolean } = {}): Promise<void> {
    const boot = options.boot === true;
    const run = this.hydrating.then(() => this.load(boot));
    this.hydrating = run;
    return run;
  }

  async reconcileAfterBoot(): Promise<void> {
    if (this.reconciled) return;
    this.reconciled = true;
    await this.ready;
    await this.reconcileInFlight().catch((error: unknown) =>
      console.warn("Could not reconcile session update deliveries", error),
    );
    for (const link of [...this.links.values()]) {
      if (link.status !== "running" || link.heldOutcome) continue;
      if (this.isHeld(link.childId)) continue;
      try {
        const child =
          this.host?.session(link.childId) ??
          (await this.host?.stored(link.childId));
        const current = this.links.get(link.childId);
        if (
          current?.status !== "running" ||
          current.generation !== link.generation
        )
          continue;
        if (!child) {
          await this.removed(link.childId);
          continue;
        }
        const interrupted = endsWithInterrupt(child);
        if (child.busy || (interrupted && this.host?.canAutoContinue(child)))
          continue;
        const update = this.outcomeUpdate(
          current,
          current.generation,
          interrupted ? "interrupted" : "settled",
          interrupted ? "" : lastAssistantTextInTurn(child),
          undefined,
          child,
        );
        await this.put(this.settled(current, update));
      } catch (error) {
        console.warn("Could not reconcile session link", link.childId, error);
      }
    }
    this.sync();
  }

  generationOf(childId: string): number | undefined {
    const link = this.links.get(childId);
    return link?.status === "running" ? link.generation : undefined;
  }

  isTracked(childId: string): boolean {
    return this.links.get(childId)?.status === "running";
  }

  parentOf(childId: string): string | undefined {
    return this.links.get(childId)?.parentId;
  }

  childrenOf(parentId: string): SessionLinkView[] {
    return [...this.links.values()]
      .filter((link) => link.parentId === parentId)
      .map((link) => ({
        childId: link.childId,
        generation: link.generation,
        status: link.status,
        held: !!link.heldOutcome,
        assignment: link.assignment,
        lastOutcome: link.lastOutcome,
        removing: link.removing,
        title: link.pending[link.pending.length - 1]?.title ?? link.childId,
        harness: link.pending[link.pending.length - 1]?.harness,
        model: link.pending[link.pending.length - 1]?.model,
        pending: [...link.pending, ...(link.inFlight?.updates ?? [])].filter(
          (update) => update.parentId === parentId,
        ).length,
      }));
  }

  linkError(parentId: string, childId: string): string | undefined {
    const link = this.links.get(childId);
    if (!link) return undefined;
    if (link.removing) return `Session ${childId} is being removed`;
    if (link.status === "running" && link.parentId !== parentId)
      return `Session ${childId} is tracked by another session until its current work ends`;
    return undefined;
  }

  async link(
    parentId: string,
    childId: string,
    requestKey: string,
    assignment?: { name?: string },
  ): Promise<() => Promise<void>> {
    const previous = this.links.get(childId);
    if (previous && previous.lastRequestKey === requestKey)
      return async () => undefined;
    // A message to an ancestor is delivered but never tracked: the two would
    // hold each other forever.
    if (this.isAncestor(childId, parentId)) return async () => undefined;
    const refused = this.linkError(parentId, childId);
    if (refused) throw new Error(refused);
    const next: SessionLink = previous
      ? {
          ...previous,
          parentId,
          generation: previous.generation + 1,
          status: "running",
          heldOutcome: undefined,
          lastRequestKey: requestKey,
          updatedAt: this.now(),
        }
      : {
          version: 1,
          childId,
          parentId,
          generation: 1,
          status: "running",
          lastRequestKey: requestKey,
          pending: [],
          updatedAt: this.now(),
        };
    if (assignment) next.assignment = {
      kind: "linked", parentId, childId, generation: next.generation, requestKey,
      ...(assignment.name ?? previous?.assignment?.name ? { name: assignment.name ?? previous?.assignment?.name } : {}),
    };
    else next.assignment = undefined;
    try {
      await this.put(next);
    } catch (error) {
      if (this.links.get(childId) === next) {
        if (previous) this.setLink(childId, previous);
        else this.deleteLink(childId);
      }
      throw error;
    }
    // Undo only what this call changed: updates recorded or put in flight
    // since then belong to the current record.
    return async () => {
      const current = this.links.get(childId);
      if (!current) return;
      if (previous) {
        await this.put({
          ...current,
          parentId: previous.parentId,
          generation: previous.generation,
          status: previous.status,
          heldOutcome: previous.heldOutcome,
          lastRequestKey: previous.lastRequestKey,
          assignment: previous.assignment,
          updatedAt: this.now(),
        });
      } else {
        this.deleteLink(childId);
        await this.store.remove(childId);
      }
    };
  }

  assignmentFor(parentId: string, childId: string, requestKey: string): AssignmentReference | undefined {
    const reference = this.links.get(childId)?.assignment;
    return reference?.parentId === parentId && reference.requestKey === requestKey ? reference : undefined;
  }

  /** Parent-initiated stop, allowed only for the caller's own child. */
  async stopFor(parentId: string, childId: string): Promise<void> {
    if (this.parentOf(childId) !== parentId)
      throw new Error(
        `Session ${childId} was not started or messaged by this session`,
      );
    await this.stop(childId);
  }

  /** Parent-initiated display name for its own child; the parent's rows are renamed by `renameLinkedAgent`. */
  async renameFor(parentId: string, childId: string, name: string): Promise<void> {
    const link = this.links.get(childId);
    if (link?.parentId !== parentId)
      throw new Error(
        `Session ${childId} was not started or messaged by this session`,
      );
    if (link.removing) throw new Error(`Session ${childId} is being removed`);
    if (!link.assignment)
      throw new Error(`Session ${childId} has no task to name`);
    if (link.assignment.name === name) return;
    const next: SessionLink = {
      ...link,
      assignment: { ...link.assignment, name },
      updatedAt: this.now(),
    };
    try {
      await this.put(next);
    } catch (error) {
      if (this.links.get(childId) === next) this.setLink(childId, link);
      throw error;
    }
  }

  /** The current display name of a linked child, with the parent it belongs to. */
  linkedName(childId: string): { parentId: string; name: string } | undefined {
    const link = this.links.get(childId);
    return link?.assignment?.name
      ? { parentId: link.parentId, name: link.assignment.name }
      : undefined;
  }

  /** The child's interrupted turn could not be continued. */
  async interrupted(childId: string): Promise<void> {
    const link = this.links.get(childId);
    if (!link || link.status !== "running") return;
    const update = this.outcomeUpdate(link, link.generation, "interrupted");
    await this.put(
      this.isHeld(childId)
        ? { ...link, heldOutcome: update, updatedAt: this.now() }
        : this.settled(link, update),
    );
    this.sync();
  }

  async stop(childId: string): Promise<void> {
    const link = this.links.get(childId);
    if (!link) return;
    await this.put({
      ...link,
      status: "idle",
      heldOutcome: undefined,
      updatedAt: this.now(),
    });
    this.sync();
  }

  async turnEnded(
    childId: string,
    generation: number,
    outcome: ControlOutcome,
  ): Promise<void> {
    this.awaitingTurnEnd.delete(childId);
    const link = this.links.get(childId);
    if (!link || link.status !== "running" || link.generation !== generation)
      return;
    const update = this.outcomeUpdate(
      link,
      generation,
      OUTCOME_STATUS[outcome.status],
      outcome.text,
      outcome.error,
    );
    await this.put(
      this.isHeld(childId)
        ? { ...link, heldOutcome: update, updatedAt: this.now() }
        : this.settled(link, update),
    );
    this.sync();
  }

  /**
   * The child was deleted. Its parent always hears once, whether the child was
   * running or idle. `session` names a child that is not open in this window.
   */
  async removed(childId: string, session?: Session): Promise<void> {
    const link = this.links.get(childId);
    if (!link || link.removing) return;
    this.awaitingTurnEnd.delete(childId);
    for (const [id, update] of this.blocked)
      if (update.childId === childId) this.blocked.delete(id);
    const update = this.outcomeUpdate(link, link.generation, "removed", "", undefined, session ?? this.host?.session(childId));
    const next: SessionLink = {
      ...link,
      removing: true,
      lastOutcome: { generation: link.generation, status: "removed" },
      generation: link.generation + 1,
      status: "idle",
      heldOutcome: undefined,
      pending: [
        ...link.pending,
        // An idle child already reported this generation's outcome under its id.
        link.status === "running" ? update : { ...update, id: `${childId}:${link.generation}:removed` },
      ],
      updatedAt: this.now(),
    };
    await this.finish(next);
    this.sync();
  }

  async parentRemoved(parentId: string): Promise<void> {
    const writes: Promise<void>[] = [];
    for (const link of [...this.links.values()]) {
      if (link.parentId === parentId) {
        this.deleteLink(link.childId);
        writes.push(this.store.remove(link.childId));
        continue;
      }
      const pending = link.pending.filter(
        (update) => update.parentId !== parentId,
      );
      const inFlight = link.inFlight?.updates.some(
        (update) => update.parentId === parentId,
      )
        ? undefined
        : link.inFlight;
      if (pending.length === link.pending.length && inFlight === link.inFlight)
        continue;
      writes.push(
        this.finish({ ...link, pending, inFlight, updatedAt: this.now() }),
      );
    }
    for (const [id, update] of this.blocked)
      if (update.parentId === parentId) this.blocked.delete(id);
    this.deliveries.delete(parentId);
    this.retries.get(parentId)?.();
    this.retries.delete(parentId);
    this.failures.delete(parentId);
    this.stalls.delete(parentId);
    this.awaitingTurnEnd.delete(parentId);
    await Promise.all(writes);
  }

  sync(): void {
    if (!this.host) return;
    try {
      this.scanBlocked();
      this.observeStalls();
      this.releaseHeld();
      this.deliver();
    } catch (error) {
      console.warn("Session link sync failed", error);
    }
  }

  private now(): number {
    return this.host?.now() ?? Date.now();
  }

  /** Apply in memory now, then persist. */
  private put(link: SessionLink): Promise<void> {
    this.setLink(link.childId, link);
    return this.store.save(link);
  }

  /** Persist, or delete a removed child's row once nothing is left to deliver. */
  private finish(link: SessionLink): Promise<void> {
    if (link.removing && !link.pending.length && !link.inFlight) {
      this.deleteLink(link.childId);
      return this.store.remove(link.childId);
    }
    return this.put(link);
  }

  private settled(link: SessionLink, update: SessionUpdate): SessionLink {
    return {
      ...link,
      status: "idle",
      heldOutcome: undefined,
      lastOutcome: { generation: update.generation, status: update.status },
      pending: [...link.pending, update],
      updatedAt: this.now(),
    };
  }

  private outcomeUpdate(
    link: SessionLink,
    generation: number,
    status: SessionUpdateStatus,
    text = "",
    error?: string,
    session = this.host?.session(link.childId),
  ): SessionUpdate {
    return {
      id: `${link.childId}:${generation}:outcome`,
      kind: "outcome",
      status,
      parentId: link.parentId,
      childId: link.childId,
      generation,
      ...(link.assignment ? { assignment: link.assignment } : {}),
      title: session?.title ?? link.childId,
      harness: session?.harness ?? "unknown",
      ...(session?.model ? { model: session.model } : {}),
      ...(text.trim() ? { excerpt: text.slice(-EXCERPT_LIMIT) } : {}),
      ...(text.length > EXCERPT_LIMIT ? { truncated: true } : {}),
      ...(error ? { error } : {}),
      at: this.now(),
    };
  }

  /**
   * A session is held while it has a running child or any undelivered update
   * addressed to it. The test is by each update's own parent, never by the
   * link that stores it, because an adopted child keeps its old updates.
   */
  /** True when `ancestor` is `id` or appears on its parent chain, whatever each link's status. */
  private isAncestor(ancestor: string, id: string): boolean {
    const seen = new Set<string>();
    for (
      let current: string | undefined = id;
      current !== undefined && !seen.has(current);
      current = this.links.get(current)?.parentId
    ) {
      if (current === ancestor) return true;
      seen.add(current);
    }
    return false;
  }

  private isHeld(id: string): boolean {
    for (const link of this.links.values()) {
      if (link.parentId === id && link.status === "running") return true;
      if (link.pending.some((update) => update.parentId === id)) return true;
      if (link.inFlight?.updates.some((update) => update.parentId === id))
        return true;
    }
    for (const update of this.blocked.values())
      if (update.parentId === id) return true;
    for (const delivery of this.deliveries.values())
      if (delivery.blocked.some((update) => update.parentId === id))
        return true;
    return false;
  }

  private scanBlocked() {
    const live = new Set<string>();
    for (const link of this.links.values()) {
      if (link.status !== "running") continue;
      const child = this.host?.session(link.childId);
      const waiting = child && pendingApprovalForSession(child);
      if (!child || !waiting) continue;
      const id = `${link.childId}:${waiting.kind}:${waiting.requestId}`;
      live.add(id);
      if (this.announced.has(id) || this.blocked.has(id)) continue;
      if (
        [...this.deliveries.values()].some((delivery) =>
          delivery.blocked.some((update) => update.id === id),
        )
      )
        continue;
      this.blocked.set(id, {
        id,
        kind: "blocked",
        status: waiting.kind,
        parentId: link.parentId,
        childId: link.childId,
        generation: link.generation,
        ...(link.assignment ? { assignment: link.assignment } : {}),
        title: child.title,
        harness: child.harness,
        model: child.model,
        requestId: waiting.requestId,
        label: waiting.label,
        at: this.now(),
      });
    }
    for (const id of this.blocked.keys())
      if (!live.has(id)) this.blocked.delete(id);
    for (const id of this.announced)
      if (!live.has(id)) this.announced.delete(id);
  }

  private observeStalls() {
    for (const [parentId, stall] of this.stalls) {
      const parent = this.host?.session(parentId);
      if (!parent) continue;
      if (parent.busy) {
        if (lastUserBlockId(parent) !== stall.userBlockId) stall.sawBusy = true;
      } else if (stall.sawBusy) {
        this.stalls.delete(parentId);
        this.failures.delete(parentId);
      }
    }
  }

  private releaseHeld() {
    for (const link of [...this.links.values()]) {
      if (link.status !== "running" || !link.heldOutcome) continue;
      if (this.awaitingTurnEnd.has(link.childId)) continue;
      if (this.host?.session(link.childId)?.busy) continue;
      if (this.isHeld(link.childId)) continue;
      const next = this.settled(link, link.heldOutcome);
      this.put(next).catch((error: unknown) => {
        console.warn("Could not release held session outcome", error);
        if (this.links.get(link.childId) === next)
          this.setLink(link.childId, link);
      });
    }
  }

  private parentIdle(parentId: string): boolean {
    const parent = this.host?.session(parentId);
    // submitSession refuses these states; each refusal would count as a retry.
    return (
      !!parent &&
      !parent.busy &&
      !parent.queuedMessages?.length &&
      !parent.pendingSwitch &&
      !isPreparingHandoff(parent)
    );
  }

  private canDeliver(parentId: string): boolean {
    return (
      this.parentIdle(parentId) &&
      !this.deliveries.has(parentId) &&
      !this.retries.has(parentId) &&
      !this.stalls.has(parentId)
    );
  }

  private deliver() {
    const parents = new Set<string>();
    for (const link of this.links.values()) {
      if (link.inFlight) continue;
      for (const update of link.pending) parents.add(update.parentId);
    }
    for (const update of this.blocked.values()) parents.add(update.parentId);
    for (const parentId of parents) {
      if (this.waking.has(parentId) || !this.canDeliver(parentId)) continue;
      this.waking.add(parentId);
      // Let session state settle before checking idle; never interrupt user input.
      this.host!.schedule(() => void this.wake(parentId), 0);
    }
  }

  private async wake(parentId: string) {
    try {
      if (!this.canDeliver(parentId)) return;
      const shares = [...this.links.values()].flatMap((link) => {
        if (link.inFlight) return [];
        const updates = link.pending.filter(
          (update) => update.parentId === parentId,
        );
        return updates.length ? [{ link, updates }] : [];
      });
      const blocked = [...this.blocked.values()].filter(
        (update) => update.parentId === parentId,
      );
      if (!shares.length && !blocked.length) return;
      const deliveryId = crypto.randomUUID();
      const written = shares.map(({ link, updates }) => {
        const next: SessionLink = {
          ...link,
          pending: link.pending.filter(
            (update) => update.parentId !== parentId,
          ),
          inFlight: { deliveryId, updates },
          updatedAt: this.now(),
        };
        this.setLink(link.childId, next);
        return { previous: link, next };
      });
      for (const update of blocked) this.blocked.delete(update.id);
      const delivery: Delivery = { deliveryId, parentId, blocked };
      this.issued.add(deliveryId);
      this.deliveries.set(parentId, delivery);
      try {
        await Promise.all(written.map(({ next }) => this.store.save(next)));
      } catch (error) {
        console.warn("Could not record session update delivery", error);
        this.deliveries.delete(parentId);
        for (const { previous, next } of written)
          if (this.links.get(previous.childId) === next)
            this.setLink(previous.childId, previous);
        return;
      }
      if (this.deliveries.get(parentId) !== delivery) return;
      if (!this.parentIdle(parentId)) {
        this.deliveries.delete(parentId);
        await this.requeue(deliveryId, false);
        return;
      }
      this.waking.delete(parentId);
      this.host!.submit(
        parentId,
        renderSessionUpdates([
          ...shares.flatMap((share) => share.updates),
          ...blocked,
        ]),
        deliveryId,
        (outcome) => {
          this.done(delivery, outcome).catch((error: unknown) =>
            console.warn("Could not record session update result", error),
          );
        },
        [...shares.flatMap((share) => share.updates), ...blocked],
      );
    } catch (error) {
      console.warn("Session update delivery failed", error);
    } finally {
      this.waking.delete(parentId);
    }
  }

  private async done(delivery: Delivery, outcome: ControlOutcome) {
    const { deliveryId, parentId } = delivery;
    if (this.deliveries.get(parentId) !== delivery) return;
    this.deliveries.delete(parentId);
    if (outcome.status !== "failed" || outcome.text.trim() !== "") {
      for (const update of delivery.blocked) this.announced.add(update.id);
      this.failures.delete(parentId);
      const parentLink = this.links.get(parentId);
      if (parentLink?.status === "running" && parentLink.heldOutcome)
        this.awaitingTurnEnd.add(parentId);
      const writes = [...this.links.values()]
        .filter((link) => link.inFlight?.deliveryId === deliveryId)
        .map((link) =>
          this.finish({ ...link, inFlight: undefined, updatedAt: this.now() }),
        );
      this.sync();
      await Promise.all(writes);
      return;
    }
    // The harness reported no output: the parent never ran this turn.
    const parent = this.host?.session(parentId);
    const shown = !!parent?.blocks.some(
      (block) => block.sessionUpdate?.deliveryId === deliveryId,
    );
    const size =
      delivery.blocked.length +
      [...this.links.values()]
        .filter((link) => link.inFlight?.deliveryId === deliveryId)
        .reduce((sum, link) => sum + link.inFlight!.updates.length, 0);
    const writes = this.requeue(deliveryId, shown);
    const failures = (this.failures.get(parentId) ?? 0) + 1;
    this.failures.set(parentId, failures);
    if (failures <= RETRY_DELAYS_MS.length) {
      const cancel = this.host!.schedule(
        () => {
          this.retries.delete(parentId);
          this.sync();
        },
        RETRY_DELAYS_MS[failures - 1],
      );
      this.retries.set(parentId, cancel);
    } else {
      this.stalls.set(parentId, {
        userBlockId: lastUserBlockId(parent),
        sawBusy: false,
      });
      const reason =
        outcome.error?.trim().replace(/\.+$/, "") ||
        "the turn ended without output";
      this.host!.notice(
        parentId,
        `Could not deliver ${size} session updates after ${RETRY_DELAYS_MS.length} attempts: ${reason}. They stay queued and will be delivered after this session's next turn.`,
      );
    }
    await writes;
  }

  /** Return an in-flight batch's outcome updates to the front of pending. */
  private requeue(deliveryId: string, repeat: boolean): Promise<void> {
    const writes = [...this.links.values()]
      .filter((link) => link.inFlight?.deliveryId === deliveryId)
      .map((link) => {
        const updates = link.inFlight!.updates.map((update) =>
          repeat ? { ...update, repeat: true } : update,
        );
        return this.put({
          ...link,
          inFlight: undefined,
          pending: [...updates, ...link.pending],
          updatedAt: this.now(),
        });
      });
    return Promise.all(writes).then(() => undefined);
  }

  private async load(boot: boolean) {
    const started = this.now();
    try {
      const rows = await this.store.load();
      const seen = new Set<string>();
      for (const row of rows) {
        seen.add(row.childId);
        const current = this.links.get(row.childId);
        if (current && current.updatedAt > row.updatedAt) continue;
        this.setLink(row.childId, row);
        if (boot && row.inFlight && !this.issued.has(row.inFlight.deliveryId))
          this.bootInFlight.add(row.inFlight.deliveryId);
      }
      for (const [childId, link] of this.links)
        if (!seen.has(childId) && link.updatedAt < started)
          this.deleteLink(childId);
      if (boot) await this.reconcileInFlight();
    } catch (error) {
      console.warn("Could not load session links", error);
    } finally {
      if (boot) this.resolveReady();
    }
    this.sync();
  }

  /**
   * Boot only. A delivery the parent shows was delivered; one it does not show
   * never reached it. A parent open elsewhere may still be running its turn,
   * so only parents open and idle here are reconciled.
   */
  private async reconcileInFlight() {
    const writes: Promise<void>[] = [];
    for (const link of [...this.links.values()]) {
      const flight = link.inFlight;
      if (!flight || !this.bootInFlight.has(flight.deliveryId)) continue;
      if (this.issued.has(flight.deliveryId)) continue;
      const parentId = flight.updates[0]?.parentId;
      const parent = parentId ? this.host?.session(parentId) : undefined;
      if (!parent || parent.busy) continue;
      this.bootInFlight.delete(flight.deliveryId);
      const delivered = parent.blocks.some(
        (block) => block.sessionUpdate?.deliveryId === flight.deliveryId,
      );
      writes.push(
        this.finish({
          ...link,
          inFlight: undefined,
          pending: delivered
            ? link.pending
            : [...flight.updates, ...link.pending],
          updatedAt: this.now(),
        }),
      );
    }
    await Promise.all(writes);
  }
}

function xml(value: string | number): string {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

function renderUpdate(update: SessionUpdate): string {
  const attributes = [
    `kind="${xml(update.kind)}"`,
    `status="${xml(update.status)}"`,
    `session="${xml(update.childId)}"`,
    `title="${xml(update.title)}"`,
    `harness="${xml(update.harness)}"`,
    `generation="${xml(update.generation)}"`,
  ];
  if (update.kind === "blocked" && update.requestId !== undefined)
    attributes.push(`request="${xml(update.requestId)}"`);
  if (update.repeat) attributes.push(`repeat="true"`);
  const lines = [`<monocode_session_update ${attributes.join(" ")}>`];
  if (update.excerpt) {
    const truncated = update.truncated || update.excerpt.length > EXCERPT_LIMIT;
    lines.push(
      `<reply${truncated ? ` truncated="true"` : ""}>${xml(update.excerpt.slice(-EXCERPT_LIMIT))}</reply>`,
    );
  }
  if (update.label) lines.push(`<label>${xml(update.label)}</label>`);
  if (update.error) lines.push(`<error>${xml(update.error)}</error>`);
  lines.push("</monocode_session_update>");
  return lines.join("\n");
}

export function renderSessionUpdates(updates: SessionUpdate[]): string {
  const ordered = [...updates].sort((a, b) => a.at - b.at);
  return [HEADER, "", ...ordered.map(renderUpdate)].join("\n");
}

/** One update as the transcript shows it, read back from the turn's text. */
export type SessionUpdateView = {
  kind: "outcome" | "blocked";
  status: SessionUpdateStatus;
  childId: string;
  title: string;
  harness: string;
  generation: number;
  requestId?: number;
  repeat: boolean;
  reply?: string;
  truncated: boolean;
  label?: string;
  error?: string;
  assignment?: AssignmentReference;
  task?: string;
  model?: string;
};

export function sessionUpdateView(update: SessionUpdate): SessionUpdateView {
  return { ...update, reply: update.excerpt, repeat: !!update.repeat, truncated: !!update.truncated };
}

const UPDATE_STATUSES = new Set<string>([
  "settled",
  "failed",
  "stopped",
  "interrupted",
  "removed",
  "approval",
  "question",
]);

/**
 * Read back the text `renderSessionUpdates` wrote. Every value in it is
 * escaped by `xml`, so no tag or quote inside a reply can end an element
 * early. Returns null for any other text, so the caller shows it as written.
 */
export function parseSessionUpdates(text: string): SessionUpdateView[] | null {
  if (!text.startsWith(HEADER)) return null;
  const views: SessionUpdateView[] = [];
  let remaining = text.slice(HEADER.length).trim();
  const validXml = (value: string) => !/[<>]|&(?!amp;|lt;|gt;|quot;|apos;)/.test(value);
  while (remaining) {
    const element = /^<monocode_session_update ([^>]*)>([\s\S]*?)<\/monocode_session_update>/.exec(remaining);
    if (!element) return null;
    const [, source, body] = element;
    const attributes: Record<string, string> = {};
    let rest = source;
    while (rest) {
      const attribute = /^([a-z_]+)="([^"]*)"(?:\s+|$)/.exec(rest);
      if (!attribute || attribute[1] in attributes || !validXml(attribute[2]) ||
          !["kind", "status", "session", "title", "harness", "generation", "request", "repeat"].includes(attribute[1])) return null;
      attributes[attribute[1]] = unxml(attribute[2]);
      rest = rest.slice(attribute[0].length);
    }
    let content = body.trim();
    const seen = new Set<string>();
    while (content) {
      const child = /^<(reply|label|error)( truncated="true")?>([\s\S]*?)<\/\1>/.exec(content);
      if (!child || seen.has(child[1]) || (child[2] && child[1] !== "reply") || !validXml(child[3])) return null;
      seen.add(child[1]);
      content = content.slice(child[0].length).trim();
    }
    remaining = remaining.slice(element[0].length).trim();
    const { kind, status, session, title, harness } = attributes;
    const generation = Number(attributes.generation);
    if (
      (kind !== "outcome" && kind !== "blocked") ||
      !UPDATE_STATUSES.has(status) ||
      !session || attributes.title === undefined || attributes.harness === undefined ||
      (kind === "blocked" ? !["approval", "question"].includes(status) || attributes.request === undefined : ["approval", "question"].includes(status) || attributes.request !== undefined) ||
      !/^\d+$/.test(attributes.generation ?? "") ||
      !Number.isSafeInteger(generation) || generation < 1 ||
      (attributes.repeat !== undefined && attributes.repeat !== "true") ||
      (attributes.request !== undefined && (!/^\d+$/.test(attributes.request) || !Number.isSafeInteger(Number(attributes.request))))
    )
      return null;
    const reply = /<reply( truncated="true")?>([\s\S]*?)<\/reply>/.exec(body);
    const label = /<label>([\s\S]*?)<\/label>/.exec(body);
    const error = /<error>([\s\S]*?)<\/error>/.exec(body);
    const requestId = Number(attributes.request);
    views.push({
      kind,
      status: status as SessionUpdateStatus,
      childId: session,
      title: title ?? "",
      harness: harness ?? "",
      generation,
      ...(Number.isSafeInteger(requestId) ? { requestId } : {}),
      repeat: attributes.repeat === "true",
      ...(reply ? { reply: unxml(reply[2]) } : {}),
      truncated: Boolean(reply?.[1]),
      ...(label ? { label: unxml(label[1]) } : {}),
      ...(error ? { error: unxml(error[1]) } : {}),
    });
  }
  return views.length > 0 ? views : null;
}

function unxml(value: string): string {
  return value
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

export function sanitizeSessionUpdates(value: unknown): SessionUpdate[] | undefined {
  if (!Array.isArray(value) || !value.length) return undefined;
  const updates: SessionUpdate[] = [];
  for (const raw of value) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return undefined;
    const row = raw as Record<string, unknown>;
    const validId = (value: unknown) => typeof value === "string" && /^[A-Za-z0-9_:-]{1,512}$/.test(value);
    if (!validId(row.id) || !validId(row.parentId) || !validId(row.childId) ||
        (row.kind !== "outcome" && row.kind !== "blocked") || typeof row.status !== "string" || !UPDATE_STATUSES.has(row.status) ||
        (row.kind === "blocked" ? !["approval", "question"].includes(row.status) || row.requestId === undefined : ["approval", "question"].includes(row.status)) ||
        !Number.isSafeInteger(row.generation) || (row.generation as number) < 1 ||
        typeof row.title !== "string" || typeof row.harness !== "string" || typeof row.at !== "number" || !Number.isFinite(row.at)) return undefined;
    const update: SessionUpdate = { id: row.id as string, parentId: row.parentId as string, childId: row.childId as string,
      kind: row.kind, status: row.status as SessionUpdateStatus, generation: row.generation as number,
      title: row.title, harness: row.harness, at: row.at };
    for (const key of ["model", "excerpt", "error", "label"] as const) {
      if (row[key] !== undefined) {
        if (typeof row[key] !== "string") return undefined;
        update[key] = row[key];
      }
    }
    for (const key of ["repeat", "truncated"] as const) {
      if (row[key] !== undefined) {
        if (typeof row[key] !== "boolean") return undefined;
        update[key] = row[key];
      }
    }
    if (row.requestId !== undefined) {
      if (!Number.isSafeInteger(row.requestId) || (row.requestId as number) < 0) return undefined;
      update.requestId = row.requestId as number;
    }
    const reference = sanitizeAcceptedReference(row.assignment);
    if (reference?.kind === "linked" && reference.parentId === update.parentId && reference.childId === update.childId && reference.generation === update.generation)
      update.assignment = reference;
    updates.push(update);
  }
  return updates;
}

type SessionLinkRow = { childId: string; parentId: string; state: string };

function parseLink(row: SessionLinkRow): SessionLink | undefined {
  try {
    const link = JSON.parse(row.state) as SessionLink;
    if (
      link.version === 1 &&
      link.childId === row.childId &&
      link.parentId === row.parentId &&
      Number.isSafeInteger(link.generation) && link.generation > 0 &&
      (link.status === "running" || link.status === "idle") &&
      typeof link.updatedAt === "number" && Number.isFinite(link.updatedAt) &&
      Array.isArray(link.pending)
    ) {
      const reference = sanitizeAcceptedReference(link.assignment);
      link.assignment = reference?.kind === "linked" && reference.parentId === link.parentId && reference.childId === link.childId && reference.requestKey === link.lastRequestKey && reference.generation === (link.removing ? link.generation - 1 : link.generation) ? reference : undefined;
      link.pending = link.pending.flatMap(update => sanitizeSessionUpdates([update]) ?? []);
      if (link.inFlight) link.inFlight.updates = sanitizeSessionUpdates(link.inFlight.updates) ?? [];
      if (link.heldOutcome) link.heldOutcome = sanitizeSessionUpdates([link.heldOutcome])?.[0];
      const terminal = link.lastOutcome;
      const lastOutcome = terminal && Number.isSafeInteger(terminal.generation) && terminal.generation > 0 &&
        UPDATE_STATUSES.has(terminal.status) && !["approval", "question"].includes(terminal.status)
        ? { generation: terminal.generation, status: terminal.status } : undefined;
      return {
        version: 1, childId: link.childId, parentId: link.parentId, generation: link.generation, status: link.status,
        ...(typeof link.lastRequestKey === "string" && /^[A-Za-z0-9_-]{1,512}$/.test(link.lastRequestKey) ? { lastRequestKey: link.lastRequestKey } : {}),
        ...(link.assignment ? { assignment: link.assignment } : {}),
        ...(lastOutcome ? { lastOutcome } : {}),
        ...(link.heldOutcome ? { heldOutcome: link.heldOutcome } : {}),
        ...(link.removing === true ? { removing: true } : {}),
        pending: link.pending,
        ...(link.inFlight && typeof link.inFlight.deliveryId === "string" && /^[A-Za-z0-9_-]{1,512}$/.test(link.inFlight.deliveryId)
          ? { inFlight: { deliveryId: link.inFlight.deliveryId, updates: link.inFlight.updates } } : {}),
        updatedAt: link.updatedAt,
      };
    }
  } catch {
    // Reported below with the row it came from.
  }
  console.warn("Skipping unreadable session link", row.childId);
  return undefined;
}

const tauriStore: SessionLinkStore = {
  load: async () => {
    const rows = await invoke<SessionLinkRow[]>("session_links_load");
    return rows.flatMap((row) => {
      const link = parseLink(row);
      return link ? [link] : [];
    });
  },
  save: (link) =>
    invoke<void>("session_link_save", {
      childId: link.childId,
      parentId: link.parentId,
      state: JSON.stringify(link),
    }),
  remove: (childId) => invoke<void>("session_link_remove", { childId }),
};

export const sessionLinks = new SessionLinks(tauriStore);
