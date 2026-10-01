// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  newSession,
  type Block,
  type Session,
} from "../../sessions/model/session";
import type { ControlOutcome } from "../../orchestration/model/orchestration";
import { pendingApprovalForSession } from "../../notifications/model/approvalToast";
import {
  SessionLinks,
  parseSessionUpdates,
  renderSessionUpdates,
  type SessionLink,
  type SessionLinkHost,
  type SessionUpdate,
} from "./sessionLinks";

type Submit = {
  parentId: string;
  text: string;
  deliveryId: string;
  done: (outcome: ControlOutcome) => void;
  updates?: SessionUpdate[];
};

const completed = (text = "done"): ControlOutcome => ({
  status: "completed",
  text,
});
const notStarted = (
  error = "agent_end before agent_settled",
): ControlOutcome => ({
  status: "failed",
  text: "",
  error,
});

function setup(rows: SessionLink[] = []) {
  const saved = new Map<string, SessionLink>();
  for (const row of rows) saved.set(row.childId, structuredClone(row));
  const store = {
    load: vi.fn(async () =>
      [...saved.values()].map((row) => structuredClone(row)),
    ),
    save: vi.fn(async (link: SessionLink) => {
      saved.set(link.childId, structuredClone(link));
    }),
    remove: vi.fn(async (childId: string) => {
      saved.delete(childId);
    }),
  };
  const open = new Map<string, Session>();
  const closed = new Map<string, Session>();
  const submits: Submit[] = [];
  /** Whether the parent shows the update block before the turn fails (Pi race). */
  const behavior = { showBlock: true };
  const deferred: (() => void)[] = [];
  const host = {
    session: (id: string) => open.get(id),
    stored: vi.fn(async (id: string) => closed.get(id)),
    canAutoContinue: vi.fn((_session: Session) => false),
    submit: vi.fn(
      (
        parentId: string,
        text: string,
        deliveryId: string,
        done: (outcome: ControlOutcome) => void,
        updates?: SessionUpdate[],
      ) => {
        const parent = open.get(parentId)!;
        parent.busy = true;
        if (behavior.showBlock)
          parent.blocks.push({
            id: crypto.randomUUID(),
            role: "user",
            text,
            sessionUpdate: { deliveryId, updates },
          } as Block);
        submits.push({ parentId, text, deliveryId, done, updates });
      },
    ),
    notice: vi.fn(),
    schedule: vi.fn((run: () => void, ms: number) => {
      if (ms === 0) {
        // Fake timers turn a 0 ms timer set inside a timer into 1 ms; keep
        // the settle deferral explicit so `tick` runs exactly these.
        let live = true;
        deferred.push(() => {
          if (live) run();
        });
        return () => {
          live = false;
        };
      }
      const timer = setTimeout(run, ms);
      return () => clearTimeout(timer);
    }),
    now: () => Date.now(),
  } satisfies SessionLinkHost;
  const links = new SessionLinks(store);
  links.bind(host);
  const add = (id: string, extra: Partial<Session> = {}) => {
    const session: Session = {
      ...newSession("claude", "/repo"),
      id,
      title: `Title ${id}`,
      ...extra,
    };
    open.set(id, session);
    return session;
  };
  /** Run the settle deferral and let the async delivery reach `submit`. */
  const tick = async () => {
    await vi.advanceTimersByTimeAsync(0);
    for (let round = 0; round < 5; round += 1) {
      for (const run of deferred.splice(0)) run();
      for (let index = 0; index < 20; index += 1) await Promise.resolve();
    }
  };
  /**
   * Settle a delivery turn in App's order: the session is idle again, then
   * `done`, then the parent's own `turnEnded` when it is a tracked child.
   */
  const settleDelivery = async (
    submit: Submit,
    outcome: ControlOutcome,
    parentGeneration?: number,
  ) => {
    open.get(submit.parentId)!.busy = false;
    submit.done(outcome);
    if (parentGeneration !== undefined)
      await links.turnEnded(submit.parentId, parentGeneration, outcome);
    await tick();
  };
  return {
    links,
    store,
    saved,
    host,
    open,
    closed,
    submits,
    behavior,
    add,
    tick,
    settleDelivery,
  };
}

function linkRow(
  extra: Partial<SessionLink> & { childId: string },
): SessionLink {
  return {
    version: 1,
    parentId: "lead",
    generation: 1,
    status: "running",
    pending: [],
    updatedAt: 1,
    ...extra,
  };
}

function outcomeRow(
  childId: string,
  extra: Partial<SessionUpdate> = {},
): SessionUpdate {
  return {
    id: `${childId}:1:outcome`,
    kind: "outcome",
    status: "settled",
    parentId: "lead",
    childId,
    generation: 1,
    title: `Title ${childId}`,
    harness: "claude",
    excerpt: `reply from ${childId}`,
    at: 1,
    ...extra,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SessionLinks", () => {
  it("1. delivers a settled outcome to an idle parent exactly once (double delivery)", async () => {
    const t = setup();
    t.add("lead");
    t.add("child");
    await t.links.link("lead", "child", "app-lead-1");
    expect(t.links.generationOf("child")).toBe(1);
    await t.links.turnEnded("child", 1, completed("All tests pass."));
    await t.tick();

    expect(t.submits).toHaveLength(1);
    expect(t.submits[0].parentId).toBe("lead");
    expect(t.submits[0].text).toContain(
      '<monocode_session_update kind="outcome" status="settled" session="child" title="Title child" harness="claude" generation="1">',
    );
    expect(t.submits[0].text).toContain("<reply>All tests pass.</reply>");
    expect(t.saved.get("child")?.inFlight?.deliveryId).toBe(
      t.submits[0].deliveryId,
    );

    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);

    await t.settleDelivery(t.submits[0], completed("Noted."));
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
    expect(t.saved.get("child")).toMatchObject({ status: "idle", pending: [] });
    expect(t.saved.get("child")?.inFlight).toBeUndefined();
  });

  it("2. waits for a busy parent and an empty queue (steering a parent mid-turn)", async () => {
    const t = setup();
    const lead = t.add("lead", { busy: true });
    t.add("child");
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.turnEnded("child", 1, completed());
    await t.tick();
    expect(t.submits).toHaveLength(0);

    lead.busy = false;
    lead.queuedMessages = [{ id: "q", text: "next", attachments: [] }];
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(0);

    lead.queuedMessages = [];
    t.links.sync();
    lead.busy = true;
    await t.tick();
    expect(t.submits).toHaveLength(0);

    lead.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
  });

  it("3. batches two outcomes into one parent turn (one turn per update)", async () => {
    const t = setup();
    const lead = t.add("lead", { busy: true });
    t.add("a");
    t.add("b");
    await t.links.link("lead", "a", "app-lead-1");
    await t.links.link("lead", "b", "app-lead-2");
    await t.links.turnEnded("a", 1, completed("from a"));
    vi.advanceTimersByTime(5);
    await t.links.turnEnded("b", 1, completed("from b"));
    lead.busy = false;
    t.links.sync();
    await t.tick();

    expect(t.submits).toHaveLength(1);
    const text = t.submits[0].text;
    expect(text).toContain('session="a"');
    expect(text).toContain('session="b"');
    expect(text.indexOf('session="a"')).toBeLessThan(
      text.indexOf('session="b"'),
    );
  });

  it("4. retries a not-started delivery after 1 s and discloses a repeat once the parent showed it (Pi race)", async () => {
    const t = setup();
    t.add("lead");
    t.add("child");
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.turnEnded("child", 1, completed("result"));
    t.behavior.showBlock = false;
    await t.tick();
    expect(t.submits).toHaveLength(1);

    await t.settleDelivery(t.submits[0], notStarted());
    expect(t.saved.get("child")?.pending).toHaveLength(1);
    expect(t.saved.get("child")?.pending[0].repeat).toBeUndefined();
    await vi.advanceTimersByTimeAsync(999);
    expect(t.submits).toHaveLength(1);
    // The second attempt commits its visible block before it fails.
    t.behavior.showBlock = true;
    await vi.advanceTimersByTimeAsync(1);
    await t.tick();
    expect(t.submits).toHaveLength(2);
    expect(t.submits[1].text).not.toContain("repeat=");

    await t.settleDelivery(t.submits[1], notStarted());
    expect(t.saved.get("child")?.pending[0].repeat).toBe(true);
    await vi.advanceTimersByTimeAsync(2000);
    await t.tick();
    expect(t.submits).toHaveLength(3);
    expect(t.submits[2].text).toContain('generation="1" repeat="true">');
  });

  it("5. does not redeliver after a failure with output or a cancelled update turn (duplicates)", async () => {
    for (const outcome of [
      { status: "failed", text: "partial answer", error: "boom" },
      { status: "cancelled", text: "" },
    ] satisfies ControlOutcome[]) {
      const t = setup();
      t.add("lead");
      t.add("child");
      await t.links.link("lead", "child", "app-lead-1");
      await t.links.turnEnded("child", 1, completed());
      await t.tick();
      await t.settleDelivery(t.submits[0], outcome);
      await vi.advanceTimersByTimeAsync(60_000);
      t.links.sync();
      await t.tick();
      expect(t.submits).toHaveLength(1);
      expect(t.saved.get("child")?.pending).toEqual([]);
      expect(t.saved.get("child")?.inFlight).toBeUndefined();
    }
  });

  it("6. stops after five retries with one notice and resumes after the parent's next turn (unbounded loop, permanent stall)", async () => {
    const t = setup();
    const lead = t.add("lead");
    t.add("child");
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.turnEnded("child", 1, completed());
    await t.tick();
    const delays = [1000, 2000, 4000, 8000, 16000];
    for (const delay of delays) {
      await t.settleDelivery(t.submits[t.submits.length - 1], notStarted());
      expect(t.host.notice).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(delay);
      await t.tick();
    }
    expect(t.submits).toHaveLength(6);

    await t.settleDelivery(t.submits[5], notStarted("Pi was not ready."));
    expect(t.host.notice).toHaveBeenCalledTimes(1);
    expect(t.host.notice).toHaveBeenCalledWith(
      "lead",
      "Could not deliver 1 session updates after 5 attempts: Pi was not ready. They stay queued and will be delivered after this session's next turn.",
    );
    expect(vi.getTimerCount()).toBe(0);
    const scheduled = t.host.schedule.mock.calls.map(([, ms]) => ms);
    expect(scheduled.filter((ms) => ms !== 0)).toEqual(delays);

    await vi.advanceTimersByTimeAsync(120_000);
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(6);

    lead.blocks.push({ id: "user-next", role: "user", text: "next" });
    lead.busy = true;
    t.links.sync();
    lead.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(7);
    expect(t.saved.get("child")?.inFlight?.updates).toHaveLength(1);
  });

  it("7. ignores a stale settlement and reports a user stop (outcome on the wrong turn)", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("child");
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.stop("child");
    expect(t.links.isTracked("child")).toBe(false);
    await t.links.link("lead", "child", "app-lead-2");
    expect(t.links.generationOf("child")).toBe(2);

    await t.links.turnEnded("child", 1, { status: "cancelled", text: "" });
    expect(t.links.isTracked("child")).toBe(true);
    expect(t.saved.get("child")?.pending).toEqual([]);

    await t.links.turnEnded("child", 2, completed("second"));
    expect(t.saved.get("child")?.pending).toEqual([
      expect.objectContaining({
        id: "child:2:outcome",
        status: "settled",
        generation: 2,
      }),
    ]);

    await t.links.link("lead", "child", "app-lead-3");
    await t.links.turnEnded("child", 3, { status: "cancelled", text: "half" });
    expect(
      t.saved.get("child")?.pending.map((update) => update.status),
    ).toEqual(["settled", "stopped"]);
  });

  it("8. keeps the generation on a retried request key (re-arming on CLI retries)", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.link("lead", "child", "app-lead-1");
    expect(t.links.generationOf("child")).toBe(1);
    expect(t.store.save).toHaveBeenCalledTimes(1);
    await t.links.link("lead", "child", "app-lead-2");
    expect(t.links.generationOf("child")).toBe(2);
  });

  it("9. holds a child's outcome until its own child's update reached it (child held forever)", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("b");
    t.add("c");
    await t.links.link("lead", "b", "app-lead-1");
    await t.links.link("b", "c", "app-b-1");
    await t.links.turnEnded("b", 1, completed("b before c"));
    expect(t.saved.get("b")).toMatchObject({
      status: "running",
      heldOutcome: expect.objectContaining({ excerpt: "b before c" }),
    });

    await t.links.turnEnded("c", 1, completed("c result"));
    await t.tick();
    expect(t.submits.map((submit) => submit.parentId)).toEqual(["b"]);
    expect(t.submits[0].text).toContain("c result");

    // App clears busy before the outer settle runs `done`, then `turnEnded`.
    await t.settleDelivery(t.submits[0], completed("b consolidated"), 1);
    expect(t.saved.get("b")).toMatchObject({ status: "idle" });
    expect(t.saved.get("b")?.heldOutcome).toBeUndefined();
    expect(t.saved.get("b")?.pending).toEqual([
      expect.objectContaining({ excerpt: "b consolidated" }),
    ]);

    t.open.get("lead")!.busy = false;
    t.links.sync();
    await t.tick();
    const toLead = t.submits.filter((submit) => submit.parentId === "lead");
    expect(toLead).toHaveLength(1);
    expect(toLead[0].text).toContain("b consolidated");
    expect(toLead[0].text).not.toContain("b before c");
  });

  it("9. releases a held outcome when the grandchild is stopped while its parent is idle", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("b");
    t.add("c");
    await t.links.link("lead", "b", "app-lead-1");
    await t.links.link("b", "c", "app-b-1");
    await t.links.turnEnded("b", 1, completed("b result"));
    expect(t.links.childrenOf("lead")[0].held).toBe(true);

    await t.links.stop("c");
    expect(t.saved.get("b")).toMatchObject({
      status: "idle",
      pending: [expect.objectContaining({ excerpt: "b result" })],
    });
  });

  it("9. reports a held child after its removed grandchild's update reached it", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("b");
    t.add("c");
    await t.links.link("lead", "b", "app-lead-1");
    await t.links.link("b", "c", "app-b-1");
    await t.links.turnEnded("b", 1, completed("b result"));
    await t.links.removed("c");
    await t.tick();
    expect(t.submits).toHaveLength(1);
    expect(t.submits[0].text).toContain('status="removed" session="c"');

    await t.settleDelivery(t.submits[0], completed("c is gone"), 1);
    expect(t.store.remove).toHaveBeenCalledWith("c");
    expect(t.saved.get("b")).toMatchObject({
      status: "idle",
      pending: [expect.objectContaining({ excerpt: "c is gone" })],
    });
  });

  it("10. reports each blocked request once and only while it is pending", async () => {
    const t = setup();
    const lead = t.add("lead");
    const child = t.add("child", { busy: true });
    await t.links.link("lead", "child", "app-lead-1");
    const approve = (requestId: number) => {
      const block: Block = {
        id: crypto.randomUUID(),
        role: "tool",
        text: "Run npm test",
        tool: { kind: "execute", title: "Run npm test" },
        approval: { requestId },
      };
      child.blocks.push(block);
      return block;
    };

    const first = approve(7);
    const label = pendingApprovalForSession(child)!.label;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
    expect(t.submits[0].text).toContain(
      'kind="blocked" status="approval" session="child" title="Title child" harness="claude" generation="1" request="7">',
    );
    expect(t.submits[0].text).toContain(`<label>${label}</label>`);
    await t.settleDelivery(t.submits[0], completed("waiting"));
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);

    // Resolved before delivery: dropped.
    first.approval!.decided = "allow";
    lead.busy = true;
    const second = approve(8);
    t.links.sync();
    second.approval!.decided = "deny";
    t.links.sync();
    lead.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);

    // A new request ID, and a resolved ID reused later.
    const third = approve(9);
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(2);
    expect(t.submits[1].text).toContain('request="9"');
    await t.settleDelivery(t.submits[1], completed("ok"));
    third.approval!.decided = "allow";
    t.links.sync();
    approve(7);
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(3);
    expect(t.submits[2].text).toContain('request="7"');

    // A failed batch never persists blocked updates; the scan re-adds them.
    t.behavior.showBlock = false;
    await t.settleDelivery(t.submits[2], notStarted());
    await vi.advanceTimersByTimeAsync(1000);
    await t.tick();
    expect(t.submits).toHaveLength(4);
    expect(t.submits[3].text).toContain('request="7"');
    for (const [link] of t.store.save.mock.calls) {
      const stored = [
        ...link.pending,
        ...(link.inFlight?.updates ?? []),
        ...(link.heldOutcome ? [link.heldOutcome] : []),
      ];
      expect(stored.every((update) => update.kind === "outcome")).toBe(true);
    }
  });

  it("10. ignores an idle child's approval", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    const child = t.add("child");
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.turnEnded("child", 1, completed());
    child.blocks.push({
      id: "approval",
      role: "tool",
      text: "Run",
      approval: { requestId: 3 },
    });
    t.open.get("lead")!.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
    expect(t.submits[0].text).not.toContain('kind="blocked"');
  });

  it("11. reconciles in-flight deliveries only at boot (lost or repeated updates after restart)", async () => {
    const delivered = linkRow({
      childId: "c1",
      status: "idle",
      inFlight: { deliveryId: "d1", updates: [outcomeRow("c1")] },
    });
    const lost = linkRow({
      childId: "c2",
      status: "idle",
      inFlight: { deliveryId: "d2", updates: [outcomeRow("c2")] },
    });
    const t = setup([delivered, lost]);
    t.add("lead", {
      blocks: [
        {
          id: "u1",
          role: "user",
          text: "update",
          sessionUpdate: { deliveryId: "d1" },
        } as Block,
      ],
    });
    await t.links.hydrate({ boot: true });
    expect(t.saved.get("c1")?.inFlight).toBeUndefined();
    expect(t.saved.get("c1")?.pending).toEqual([]);
    expect(t.saved.get("c2")?.inFlight).toBeUndefined();
    expect(t.saved.get("c2")?.pending.map((update) => update.id)).toEqual([
      "c2:1:outcome",
    ]);
    await t.tick();
    expect(t.submits).toHaveLength(1);
    expect(t.submits[0].text).toContain('session="c2"');
    expect(t.submits[0].text).not.toContain('session="c1"');

    // A non-boot hydrate never reconciles.
    const other = setup([
      linkRow({
        childId: "c3",
        status: "idle",
        inFlight: { deliveryId: "d3", updates: [outcomeRow("c3")] },
      }),
    ]);
    other.add("lead");
    await other.links.hydrate();
    expect(other.saved.get("c3")?.inFlight?.deliveryId).toBe("d3");
    await other.tick();
    expect(other.submits).toHaveLength(0);
  });

  it("11. never reconciles a delivery this window issued, and keeps a newer in-memory record", async () => {
    const t = setup();
    const lead = t.add("lead");
    t.add("child");
    t.behavior.showBlock = false;
    await t.links.link("lead", "child", "app-lead-1");
    await t.links.turnEnded("child", 1, completed());
    await t.tick();
    expect(t.submits).toHaveLength(1);
    lead.busy = false;
    await t.links.hydrate({ boot: true });
    expect(t.saved.get("child")?.inFlight?.deliveryId).toBe(
      t.submits[0].deliveryId,
    );

    vi.advanceTimersByTime(10);
    await t.links.link("lead", "fresh", "app-lead-2");
    t.saved.set(
      "fresh",
      linkRow({ childId: "fresh", generation: 7, updatedAt: 0 }),
    );
    await t.links.hydrate();
    expect(t.links.generationOf("fresh")).toBe(1);
  });

  it("12. reconciles running links once after boot (lost interrupted work)", async () => {
    const t = setup([
      linkRow({ childId: "busy" }),
      linkRow({ childId: "resumes" }),
      linkRow({ childId: "interrupted" }),
      linkRow({ childId: "finished" }),
      linkRow({ childId: "closed" }),
      linkRow({ childId: "missing" }),
      linkRow({
        childId: "held",
        heldOutcome: outcomeRow("held", { excerpt: "held reply" }),
      }),
      linkRow({ childId: "grandchild", parentId: "held" }),
    ]);
    const interrupt: Block = {
      id: "int",
      role: "system",
      text: "Turn interrupted when MonoCode quit.",
      notice: "interrupt",
    };
    t.add("lead", { busy: true });
    t.add("busy", { busy: true });
    const resumes = t.add("resumes", { blocks: [interrupt] });
    t.add("interrupted", { blocks: [interrupt] });
    t.add("finished", {
      blocks: [
        { id: "u", role: "user", text: "go" },
        { id: "a", role: "assistant", text: "final answer" },
        { id: "s", role: "system", text: "status" },
      ],
    });
    t.closed.set("closed", {
      ...newSession("codex", "/repo"),
      id: "closed",
      title: "Closed",
      blocks: [{ id: "a", role: "assistant", text: "closed answer" }],
    });
    t.add("held");
    t.add("grandchild", { busy: true });
    t.host.canAutoContinue.mockImplementation((session) => session === resumes);

    expect(t.links.generationOf("busy")).toBeUndefined();
    void t.links.hydrate({ boot: true });
    await t.links.ready;
    expect(t.links.generationOf("busy")).toBe(1);

    await t.links.reconcileAfterBoot();
    const status = (id: string) =>
      t.saved.get(id)?.pending.map((update) => update.status);
    expect(t.links.isTracked("busy")).toBe(true);
    expect(t.links.isTracked("resumes")).toBe(true);
    expect(status("interrupted")).toEqual(["interrupted"]);
    expect(t.saved.get("finished")?.pending[0]).toMatchObject({
      status: "settled",
      excerpt: "final answer",
    });
    expect(t.saved.get("closed")?.pending[0]).toMatchObject({
      status: "settled",
      excerpt: "closed answer",
      title: "Closed",
      harness: "codex",
    });
    expect(t.saved.get("missing")).toMatchObject({
      removing: true,
      pending: [expect.objectContaining({ status: "removed" })],
    });
    expect(t.saved.get("held")).toMatchObject({
      status: "running",
      pending: [],
    });
    expect(t.saved.get("held")?.heldOutcome?.excerpt).toBe("held reply");

    await t.links.reconcileAfterBoot();
    expect(t.host.stored).toHaveBeenCalledTimes(2);

    // After `ready`, a turn end at the persisted generation records an outcome.
    await t.links.turnEnded(
      "busy",
      t.links.generationOf("busy")!,
      completed("after boot"),
    );
    expect(status("busy")).toEqual(["settled"]);
  });

  it("13. enforces one lineage per child and delivers old updates to the old parent (wrong parent)", async () => {
    const t = setup();
    const lead = t.add("lead", { busy: true });
    t.add("other", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    const message =
      "Session c is tracked by another session until its current work ends";
    expect(t.links.linkError("other", "c")).toBe(message);
    await expect(t.links.link("other", "c", "app-other-1")).rejects.toThrow(
      message,
    );
    expect(t.links.parentOf("c")).toBe("lead");

    await t.links.turnEnded("c", 1, completed("for lead"));
    expect(t.links.linkError("other", "c")).toBeUndefined();
    await t.links.link("other", "c", "app-other-2");
    expect(t.saved.get("c")).toMatchObject({
      parentId: "other",
      generation: 2,
      status: "running",
    });

    lead.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits.map((submit) => submit.parentId)).toEqual(["lead"]);
    expect(t.submits[0].text).toContain("for lead");

    await t.links.removed("c");
    expect(t.links.linkError("lead", "c")).toBe("Session c is being removed");
  });

  it("13. holds by each update's own parent, not by the link that stores it", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("b", { busy: true });
    t.add("x", { busy: true });
    t.add("c");
    await t.links.link("lead", "b", "app-lead-1");
    await t.links.link("lead", "x", "app-lead-2");
    await t.links.link("b", "c", "app-b-1");
    await t.links.turnEnded("c", 1, completed("for b"));
    await t.links.link("x", "c", "app-x-1");
    await t.links.stop("c");
    expect(t.saved.get("c")).toMatchObject({
      parentId: "x",
      pending: [expect.objectContaining({ parentId: "b" })],
    });

    await t.links.turnEnded("b", 1, completed("b reply"));
    await t.links.turnEnded("x", 1, completed("x reply"));
    expect(t.saved.get("b")).toMatchObject({ status: "running" });
    expect(t.saved.get("b")?.heldOutcome?.excerpt).toBe("b reply");
    expect(t.saved.get("x")).toMatchObject({
      status: "idle",
      pending: [expect.objectContaining({ excerpt: "x reply" })],
    });
  });

  it("14. reports a removed child, then deletes its row; parent removal drops silently", async () => {
    const t = setup();
    const lead = t.add("lead");
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    await t.links.removed("c");
    await t.links.turnEnded("c", 1, { status: "cancelled", text: "" });
    expect(t.saved.get("c")?.pending.map((update) => update.status)).toEqual([
      "removed",
    ]);
    await t.tick();
    expect(t.submits).toHaveLength(1);
    expect(t.submits[0].text).toContain('status="removed" session="c"');
    expect(t.store.remove).not.toHaveBeenCalled();
    await t.settleDelivery(t.submits[0], completed("ok"));
    expect(t.store.remove).toHaveBeenCalledWith("c");
    expect(t.links.parentOf("c")).toBeUndefined();

    lead.busy = true;
    t.add("d");
    t.add("e");
    await t.links.link("lead", "d", "app-lead-2");
    await t.links.link("lead", "e", "app-lead-3");
    await t.links.turnEnded("d", 1, completed());
    await t.links.turnEnded("e", 1, completed());
    await t.links.link("other", "e", "app-other-1");
    await t.links.parentRemoved("lead");
    expect(t.store.remove).toHaveBeenCalledWith("d");
    expect(t.saved.get("e")).toMatchObject({ parentId: "other", pending: [] });
    lead.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
  });

  it("14. reports an idle child's deletion once, named by its task, then deletes its row", async () => {
    const t = setup();
    t.add("lead");
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1", { name: "Reviewer" });
    await t.links.turnEnded("c", 1, completed("first"));
    await t.tick();
    await t.settleDelivery(t.submits[0], completed("ok"));
    expect(t.saved.get("c")).toMatchObject({ status: "idle", pending: [] });

    // Not open here: the caller hands over the stored session for its title.
    const child = t.open.get("c")!;
    t.open.delete("c");
    await t.links.removed("c", { ...child, title: "Stored child", harness: "codex" });
    await t.links.removed("c");
    await t.tick();
    expect(t.submits).toHaveLength(2);
    expect(t.submits[1].text).toContain('status="removed" session="c" title="Stored child" harness="codex"');
    expect(t.submits[1].updates).toEqual([
      expect.objectContaining({
        status: "removed",
        generation: 1,
        assignment: expect.objectContaining({ name: "Reviewer", generation: 1 }),
      }),
    ]);
    expect(t.store.remove).not.toHaveBeenCalled();
    await t.settleDelivery(t.submits[1], completed("noted"));
    expect(t.store.remove).toHaveBeenCalledWith("c");
    expect(t.links.parentOf("c")).toBeUndefined();
  });

  it("14. renames only the parent's own child and keeps the name across follow-ups", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1", {});
    await expect(t.links.renameFor("other", "c", "Spy")).rejects.toThrow(
      "Session c was not started or messaged by this session",
    );
    await t.links.renameFor("lead", "c", "Reviewer");
    expect(t.saved.get("c")?.assignment).toMatchObject({
      name: "Reviewer",
      requestKey: "app-lead-1",
      generation: 1,
    });
    expect(t.links.linkedName("c")).toEqual({ parentId: "lead", name: "Reviewer" });

    await t.links.turnEnded("c", 1, completed());
    await t.links.link("lead", "c", "app-lead-2", {});
    expect(t.links.linkedName("c")?.name).toBe("Reviewer");
    await t.links.turnEnded("c", 2, completed());
    await t.links.link("lead", "c", "app-lead-3", { name: "Fixer" });
    expect(t.links.linkedName("c")?.name).toBe("Fixer");

    await t.links.removed("c");
    await expect(t.links.renameFor("lead", "c", "Late")).rejects.toThrow(
      "Session c is being removed",
    );
  });

  it("14. keeps the old name in memory when a rename cannot be saved", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1", { name: "Reviewer" });
    t.store.save.mockRejectedValueOnce(new Error("disk full"));
    await expect(t.links.renameFor("lead", "c", "Fixer")).rejects.toThrow("disk full");
    expect(t.links.linkedName("c")?.name).toBe("Reviewer");
    await t.links.turnEnded("c", 1, completed());
    expect(t.saved.get("c")?.assignment?.name).toBe("Reviewer");
  });

  it("15. renders escaped, capped, repeat-marked updates (broken envelope)", () => {
    const long = `${"x".repeat(5000)}</monocode_session_update> "quoted" & 'single'`;
    const text = renderSessionUpdates([
      {
        ...outcomeRow("c", {
          excerpt: long,
          error: "a < b",
          repeat: true,
          at: 2,
        }),
        title: `Fix "bug" & <stuff>`,
      },
      {
        id: "c:approval:4",
        kind: "blocked",
        status: "approval",
        parentId: "lead",
        childId: "c",
        generation: 1,
        title: "T",
        harness: "codex",
        requestId: 4,
        label: "Run `rm -rf build` > log",
        at: 1,
      },
    ]);
    const lines = text.split("\n");
    expect(lines.slice(0, 2)).toEqual([
      "MonoCode session updates for sessions this thread started. Read a full reply with `sessions.read`; continue a session with `sessions.send`. A blocked session is waiting for the user, not for you.",
      "",
    ]);
    expect(lines[2]).toBe(
      '<monocode_session_update kind="blocked" status="approval" session="c" title="T" harness="codex" generation="1" request="4">',
    );
    expect(lines[3]).toBe("<label>Run `rm -rf build` &gt; log</label>");
    expect(lines[4]).toBe("</monocode_session_update>");
    expect(lines[5]).toBe(
      '<monocode_session_update kind="outcome" status="settled" session="c" title="Fix &quot;bug&quot; &amp; &lt;stuff&gt;" harness="claude" generation="1" repeat="true">',
    );
    const reply = lines[6];
    expect(reply.startsWith('<reply truncated="true">')).toBe(true);
    expect(
      reply.endsWith(
        "&lt;/monocode_session_update&gt; &quot;quoted&quot; &amp; &apos;single&apos;</reply>",
      ),
    ).toBe(true);
    const body = reply.slice(
      '<reply truncated="true">'.length,
      -"</reply>".length,
    );
    const raw = body
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .replace(/&apos;/g, "'")
      .replace(/&amp;/g, "&");
    expect(raw).toBe(long.slice(-4000));
    expect(lines[7]).toBe("<error>a &lt; b</error>");
    expect(lines[8]).toBe("</monocode_session_update>");
    expect(text.match(/<\/monocode_session_update>/g)).toHaveLength(2);
  });

  it("15. caps a long turn reply at 4000 characters when recording it", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    await t.links.turnEnded(
      "c",
      1,
      completed(`${"a".repeat(10)}${"b".repeat(4000)}`),
    );
    expect(t.saved.get("c")?.pending[0]).toMatchObject({
      excerpt: "b".repeat(4000),
      truncated: true,
    });
  });

  it("never tracks a message to an ancestor (two sessions holding each other)", async () => {
    const t = setup();
    t.add("a");
    t.add("b");
    t.add("c");
    await t.links.link("a", "b", "app-a-1");
    await t.links.link("b", "c", "app-b-1");
    await t.links.link("b", "a", "app-b-2");
    await t.links.link("c", "a", "app-c-1");
    expect(t.links.parentOf("a")).toBeUndefined();
    expect(t.saved.has("a")).toBe(false);

    await t.links.stop("c");
    await t.links.turnEnded("b", 1, completed("b reply"));
    expect(t.links.childrenOf("a")).toEqual([
      expect.objectContaining({ childId: "b", held: false, status: "idle" }),
    ]);
    await t.tick();
    expect(t.submits.map((submit) => submit.parentId)).toEqual(["a"]);
    expect(t.submits[0].text).toContain("b reply");
  });

  it("rolls back only the link fields, keeping a batch already in flight (double delivery)", async () => {
    const t = setup();
    const lead = t.add("lead", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    await t.links.turnEnded("c", 1, completed("first"));
    const rollback = await t.links.link("lead", "c", "app-lead-2");
    lead.busy = false;
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
    const deliveryId = t.submits[0].deliveryId;

    await rollback();
    expect(t.saved.get("c")).toMatchObject({
      generation: 1,
      status: "idle",
      lastRequestKey: "app-lead-1",
      inFlight: { deliveryId },
      pending: [],
    });
    await t.settleDelivery(t.submits[0], completed("noted"));
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
  });

  it("waits while the parent switches providers or prepares a handoff (wasted retries)", async () => {
    const t = setup();
    const lead = t.add("lead", {
      pendingSwitch: {
        from: "codex",
        fromModel: "codex:test",
        fromSettings: {},
      },
    });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    await t.links.turnEnded("c", 1, completed());
    await t.tick();
    expect(t.submits).toHaveLength(0);

    lead.pendingSwitch = undefined;
    lead.blocks.push({
      id: "handoff",
      role: "handoff",
      text: "",
      handoff: { status: "preparing" },
    } as Block);
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(0);
    expect(
      t.host.schedule.mock.calls.filter(([, ms]) => ms !== 0),
    ).toHaveLength(0);

    lead.blocks.pop();
    t.links.sync();
    await t.tick();
    expect(t.submits).toHaveLength(1);
  });

  it("records an interrupted outcome when a tracked turn cannot continue (running forever)", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("b");
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    await t.links.interrupted("c");
    expect(t.saved.get("c")).toMatchObject({
      status: "idle",
      pending: [
        expect.objectContaining({
          id: "c:1:outcome",
          status: "interrupted",
          generation: 1,
        }),
      ],
    });
    await t.links.interrupted("c");
    expect(t.saved.get("c")?.pending).toHaveLength(1);

    await t.links.link("lead", "b", "app-lead-2");
    await t.links.link("b", "grandchild", "app-b-1");
    await t.links.interrupted("b");
    expect(t.saved.get("b")).toMatchObject({
      status: "running",
      heldOutcome: expect.objectContaining({ status: "interrupted" }),
      pending: [],
    });
  });

  it("stops a child only for its own parent (stopping another thread's work)", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1");
    const message = "Session c was not started or messaged by this session";
    await expect(t.links.stopFor("other", "c")).rejects.toThrow(message);
    await expect(t.links.stopFor("lead", "missing")).rejects.toThrow(
      "Session missing was not started or messaged by this session",
    );
    expect(t.links.isTracked("c")).toBe(true);
    expect(t.saved.get("c")?.status).toBe("running");

    await t.links.stopFor("lead", "c");
    expect(t.saved.get("c")).toMatchObject({ status: "idle", pending: [] });
  });
});

describe("native assignment generation attribution", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("freezes old update references through replay, follow-up and adoption and restores them on rejection", async () => {
    const t = setup();
    t.add("lead", { busy: true });
    t.add("other", { busy: true });
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1", { name: "Reviewer" });
    const first = t.links.assignmentFor("lead", "c", "app-lead-1");
    await t.links.turnEnded("c", 1, completed("first"));
    await t.links.link("lead", "c", "app-lead-1", { name: "Must not rename" });
    expect(t.links.assignmentFor("lead", "c", "app-lead-1")).toEqual(first);
    expect(t.links.isTracked("c")).toBe(false);
    await t.links.link("lead", "c", "app-lead-2", {});
    expect(t.saved.get("c")?.assignment).toMatchObject({ generation: 2, name: "Reviewer", requestKey: "app-lead-2" });
    await t.links.turnEnded("c", 1, completed("late stale settlement"));
    expect(t.links.isTracked("c")).toBe(true);
    await t.links.turnEnded("c", 2, completed("second"));
    const rollback = await t.links.link("other", "c", "app-other-1", {});
    expect(t.saved.get("c")?.pending.map(update => update.assignment)).toEqual([first, expect.objectContaining({ parentId: "lead", generation: 2 })]);
    expect(t.saved.get("c")?.assignment).toMatchObject({ parentId: "other", generation: 3, name: "Reviewer" });
    await rollback();
    expect(t.saved.get("c")?.assignment).toMatchObject({ parentId: "lead", generation: 2 });
    expect(t.saved.get("c")?.lastOutcome).toEqual({ generation: 2, status: "settled" });
    expect(JSON.stringify(t.saved.get("c"))).not.toContain('"task"');
  });

  it("delivers typed frozen references and keeps terminal status after delivery", async () => {
    const t = setup();
    t.add("lead");
    t.add("c");
    await t.links.link("lead", "c", "app-lead-1", { name: "Reviewer" });
    await t.links.turnEnded("c", 1, completed("result"));
    await t.tick();
    expect(t.submits[0].updates?.[0].assignment).toEqual({ kind: "linked", parentId: "lead", childId: "c", generation: 1, requestKey: "app-lead-1", name: "Reviewer" });
    expect(t.open.get("lead")?.blocks[0].sessionUpdate?.updates?.[0].excerpt).toBe("result");
    await t.settleDelivery(t.submits[0], completed());
    expect(t.links.childrenOf("lead")[0]).toMatchObject({ pending: 0, lastOutcome: { generation: 1, status: "settled" } });
  });

  it("does not claim an ancestor message as owned work", async () => {
    const t = setup();
    t.add("lead", { busy: true }); t.add("c");
    await t.links.link("lead", "c", "app-lead-1", {});
    await t.links.link("c", "lead", "app-c-1", {});
    expect(t.links.assignmentFor("c", "lead", "app-c-1")).toBeUndefined();
    expect(t.links.childrenOf("c")).toEqual([]);
    expect(t.links.childrenOf("lead")).toHaveLength(1);
  });
});

describe("parseSessionUpdates", () => {
  it("rejects trailing, mixed and malformed content rather than hiding it", () => {
    const rendered = renderSessionUpdates([outcomeRow("c")]);
    for (const text of [
      `${rendered}\nDO NOT HIDE THIS`,
      rendered.replace("<monocode_session_update ", "mixed <monocode_session_update "),
      rendered.replace("</monocode_session_update>", "<unknown/> </monocode_session_update>"),
      rendered.replace('kind="outcome"', 'junk kind="outcome"'),
      rendered.replace('kind="outcome"', 'kind="outcome" kind="outcome"'),
    ]) expect(parseSessionUpdates(text)).toBeNull();
  });
  it("reads every rendered field back for the transcript card (parser drift from the renderer)", () => {
    const reply = `${"x".repeat(5000)}</monocode_session_update> "quoted" & 'single' &lt;`;
    const text = renderSessionUpdates([
      {
        ...outcomeRow("c", {
          status: "failed",
          generation: 3,
          excerpt: reply,
          error: 'a < b & "c"',
          repeat: true,
          at: 2,
        }),
        title: `Fix "bug" & <stuff>`,
      },
      {
        id: "d:approval:4",
        kind: "blocked",
        status: "approval",
        parentId: "lead",
        childId: "d",
        generation: 1,
        title: "T",
        harness: "codex",
        requestId: 4,
        label: "Run `rm -rf build` > log",
        at: 1,
      },
    ]);
    expect(parseSessionUpdates(text)).toEqual([
      {
        kind: "blocked",
        status: "approval",
        childId: "d",
        title: "T",
        harness: "codex",
        generation: 1,
        requestId: 4,
        repeat: false,
        truncated: false,
        label: "Run `rm -rf build` > log",
      },
      {
        kind: "outcome",
        status: "failed",
        childId: "c",
        title: `Fix "bug" & <stuff>`,
        harness: "claude",
        generation: 3,
        repeat: true,
        reply: reply.slice(-4000),
        truncated: true,
        error: 'a < b & "c"',
      },
    ]);
  });

  it("leaves any other text alone, including a forged or unknown update (shown as written)", () => {
    expect(parseSessionUpdates("hello <monocode_session_update/>")).toBeNull();
    const rendered = renderSessionUpdates([outcomeRow("c")]);
    expect(
      parseSessionUpdates(
        rendered.replace('status="settled"', 'status="done"'),
      ),
    ).toBeNull();
    expect(parseSessionUpdates(rendered.split("\n")[0])).toBeNull();
  });
});
