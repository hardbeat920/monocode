// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  SessionLinks,
  renderSessionUpdates,
  type SessionUpdate,
  type SessionUpdateStatus,
} from "../../agent-app/model/sessionLinks";
import {
  renameLinkedAgent,
  type AssignmentReceipt,
} from "../../agent-app/model/assignments";
import type { Block, Session } from "../model/session";
import { resetHarnessModelOverlays, setHarnessModels } from "../model/models";
import { AgentTranscript } from "./AgentTranscript";

const copyMessage = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock("../../../platform/tauri/clipboard", () => ({ copyMessage }));

const CHILD = "app-fbb3a464-3907-4dff-bae7-c7250adff5ef-b067a560";

function updateBlock(text: string, updates?: SessionUpdate[]): Block {
  return {
    id: "update",
    role: "user",
    text,
    sessionUpdate: { deliveryId: "delivery-1", ...(updates ? { updates } : {}) },
  };
}

function update(
  status: SessionUpdateStatus,
  fields: Partial<SessionUpdate> = {},
): SessionUpdate {
  const blocked = status === "approval" || status === "question";
  return {
    id: `${status}:1`,
    kind: blocked ? "blocked" : "outcome",
    status,
    parentId: "lead",
    childId: `child-${status}`,
    generation: 1,
    title: `Agent ${status}`,
    harness: "claude",
    ...(blocked ? { requestId: 7 } : {}),
    at: 1,
    ...fields,
  };
}

function receipt(fields: Partial<AssignmentReceipt> = {}): Block {
  return {
    id: `receipt-${fields.generation ?? 1}`,
    role: "system",
    text: "",
    assignmentReceipt: {
      kind: "linked",
      parentId: "lead",
      childId: CHILD,
      generation: 1,
      requestKey: CHILD,
      name: "luna-hi",
      harness: "claude",
      model: "claude:haiku-4.5",
      task: "Write a funny story about a goose",
      ...fields,
    } as AssignmentReceipt,
  };
}

function session(fields: Partial<Session> & { id: string }): Session {
  return {
    harness: "claude",
    model: "claude:haiku-4.5",
    modelSettings: {},
    runtimeMode: "supervised",
    title: "",
    cwd: "/repo",
    blocks: [],
    ...fields,
  };
}

const rendered = renderSessionUpdates([
  update("settled", { childId: "child-a", title: "Greet the operator", harness: "pi", excerpt: "Hi, Claude <3" }),
  update("approval", { childId: "child-b", generation: 2, title: "Clean the build", label: "Approve: rm -rf build" }),
  update("removed", { childId: "child-c", title: "Gone", harness: "codex" }),
]);

describe("operator agent rows", () => {
  let root: Root;
  let container: HTMLDivElement;
  const onOpenSession = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    onOpenSession.mockClear();
    copyMessage.mockClear();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    resetHarnessModelOverlays();
  });

  async function render(
    blocks: Block[],
    props: Partial<Parameters<typeof AgentTranscript>[0]> = {},
  ) {
    await act(async () =>
      root.render(
        createElement(AgentTranscript, {
          blocks,
          busy: false,
          onOpenSession,
          ...props,
        }),
      ),
    );
  }

  function row(name: string): HTMLElement {
    const found = [...container.querySelectorAll<HTMLElement>("[data-agent-row]")].find(
      (element) => element.textContent?.includes(name),
    );
    if (!found) throw new Error(`No agent row for ${name}`);
    return found;
  }

  async function open(name: string): Promise<HTMLElement> {
    const target = row(name);
    const toggle = target.querySelector<HTMLButtonElement>("button[aria-expanded]");
    if (!toggle) throw new Error(`${name} has nothing to open`);
    await act(async () => toggle.click());
    return target;
  }

  function button(scope: HTMLElement, text: string): HTMLButtonElement {
    const found = [...scope.querySelectorAll("button")].find(
      (element) => element.textContent === text,
    );
    if (!found) throw new Error(`No button ${text}`);
    return found;
  }

  describe("task sent", () => {
    it("is one collapsed row with a readable model; the task and raw ids only show when opened", async () => {
      await render([receipt()]);
      const collapsed = row("luna-hi");
      expect(collapsed.textContent).toContain("Haiku 4.5");
      expect(collapsed.textContent).not.toContain(CHILD);
      expect(collapsed.textContent).not.toContain("claude:haiku-4.5");
      expect(collapsed.textContent).not.toContain("goose");
      expect(container.textContent).not.toContain("Task accepted");
      expect(container.textContent).not.toContain("Request");

      const opened = await open("luna-hi");
      expect(opened.textContent).toContain("Write a funny story about a goose");
      const ids = opened.querySelector<HTMLElement>("p[title]");
      expect(ids?.title).toBe(CHILD);
      expect(ids?.textContent).toBe(
        "New session · Claude Code · claude:haiku-4.5 · app-fbb3a464…5ef-b067a560",
      );
      await act(async () => button(opened, "Open luna-hi").click());
      expect(onOpenSession).toHaveBeenCalledWith(CHILD);
    });

    it("reads a follow-up as follow-up, keeping Request N for the opened ids", async () => {
      await render([receipt({ generation: 3, requestKey: "app-lead-9" })]);
      expect(row("luna-hi").textContent).toContain("follow-up");
      expect(container.textContent).not.toContain("Request 3");
      const opened = await open("luna-hi");
      expect(opened.querySelector("p[title]")?.textContent).toMatch(/^Request 3 · claude:haiku-4\.5 · /);
    });

    it("names an unnamed child by its title from history, keeping the raw id for the ids line", async () => {
      await render([receipt({ name: undefined })], {
        sessionFor: (id) =>
          id === CHILD ? { id, title: "Goose stories", harness: "claude", model: "claude:haiku-4.5" } : undefined,
      });
      const collapsed = row("Goose stories");
      expect(collapsed.textContent).not.toContain(CHILD);
      expect((await open("Goose stories")).querySelector("p[title]")?.getAttribute("title")).toBe(CHILD);
    });

    it("renames existing task and update rows from the live link, only for the parent that owns it", async () => {
      const settled = update("settled", { childId: CHILD, excerpt: "Done" });
      const blocks = [receipt(), updateBlock(renderSessionUpdates([settled]), [settled])];
      const linkedTo = (parentId: string) => (id: string) =>
        id === CHILD
          ? { id, title: "Goose", harness: "claude" as const, model: "claude:haiku-4.5", linkedName: { parentId, name: "luna-lo" } }
          : undefined;
      const names = () =>
        [...container.querySelectorAll<HTMLElement>("[data-agent-row]")].map((element) =>
          element.textContent?.includes("luna-lo") ? "luna-lo" : element.textContent?.includes("luna-hi") ? "luna-hi" : "other",
        );
      await render(blocks, { sessionFor: linkedTo("other-parent") });
      expect(names()).toEqual(["luna-hi", "other"]);
      await render(blocks, { sessionFor: linkedTo("lead") });
      expect(names()).toEqual(["luna-lo", "luna-lo"]);
    });

    it("keeps a rename on earlier rows after the deleted child's link is gone", async () => {
      const lead = session({ id: "lead", blocks: [receipt()] });
      const links = new SessionLinks({ load: async () => [], save: async () => {}, remove: async () => {} });
      const deliveries: ((outcome: { status: "completed"; text: string }) => void)[] = [];
      links.bind({
        session: (id) => (id === "lead" ? lead : undefined),
        stored: async () => undefined,
        canAutoContinue: () => false,
        submit: (_parentId, text, deliveryId, done, updates) => {
          lead.blocks = [...lead.blocks, { id: deliveryId, role: "user", text, sessionUpdate: { deliveryId, updates } }];
          deliveries.push(done);
        },
        notice: () => {},
        schedule: (run) => {
          void Promise.resolve().then(run);
          return () => {};
        },
        now: () => 1,
      });
      const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
      const deliver = async () => {
        links.sync();
        await settle();
        deliveries.shift()!({ status: "completed", text: "ok" });
        await settle();
      };
      await links.link("lead", CHILD, CHILD, { name: "luna-hi" });
      await links.turnEnded(CHILD, 1, { status: "completed", text: "Done" });
      await deliver();
      // What the host does for sessions.rename, then the child is deleted.
      await links.renameFor("lead", CHILD, "luna-lo");
      lead.blocks = renameLinkedAgent(lead.blocks, "lead", CHILD, "luna-lo");
      await links.removed(CHILD);
      await deliver();
      expect(links.linkedName(CHILD)).toBeUndefined();
      await render(lead.blocks, { sessionFor: () => undefined });
      const rows = [...container.querySelectorAll<HTMLElement>("[data-agent-row]")];
      expect(rows).toHaveLength(3);
      for (const element of rows) {
        expect(element.textContent).toContain("luna-lo");
        expect(element.textContent).not.toContain("luna-hi");
      }
    });

    it("shows the raw model id until the catalog can place it, then its name, without naming the default", async () => {
      await render([receipt({ harness: "pi", model: "pi:openai-codex/gpt-6-luna" })]);
      expect(row("luna-hi").textContent).toContain("pi:openai-codex/gpt-6-luna");
      expect(row("luna-hi").textContent).not.toContain("Default");
      await act(async () =>
        setHarnessModels("pi", [{ id: "pi:openai-codex/gpt-6-luna", harness: "pi", name: "GPT-6 Luna" }]),
      );
      expect(row("luna-hi").textContent).toContain("GPT-6 Luna");
      expect(row("luna-hi").textContent).not.toContain("pi:openai-codex");
    });

    it("stays working through a steer and the child's own session update", async () => {
      const reference = { ...receipt().assignmentReceipt!, task: undefined };
      const child = session({
        id: CHILD,
        busy: true,
        blocks: [
          { id: "task", role: "user", text: "goose", acceptedAssignment: reference as never, startedAt: Date.now() - 42_000 },
          { id: "a", role: "assistant", text: "Writing" },
          { id: "steer", role: "user", text: "make Gerald grumpier" },
          { id: "grandchild", role: "user", text: "update", startedAt: Date.now() - 5_000, sessionUpdate: { deliveryId: "d" } },
        ],
      });
      await render([receipt()], { sessionFor: () => child });
      expect(row("luna-hi").querySelector(".shimmer-text")?.textContent).toBe("luna-hi");
      expect(row("luna-hi").textContent).toContain("working 42s");
    });

    it("stops working on the task once the user starts a turn of their own", async () => {
      const reference = { ...receipt().assignmentReceipt!, task: undefined };
      const child = session({
        id: CHILD,
        busy: true,
        blocks: [
          { id: "task", role: "user", text: "goose", acceptedAssignment: reference as never, startedAt: 1 },
          { id: "human", role: "user", text: "something else", startedAt: 2 },
        ],
      });
      await render([receipt()], { sessionFor: () => child });
      expect(row("luna-hi").querySelector(".shimmer-text")).toBeNull();
      expect(row("luna-hi").textContent).not.toContain("working");
    });

    it("says working without a clock when the task has no start time", async () => {
      const reference = { ...receipt().assignmentReceipt!, task: undefined };
      const child = session({
        id: CHILD,
        busy: true,
        blocks: [{ id: "task", role: "user", text: "goose", acceptedAssignment: reference as never }],
      });
      await render([receipt()], { sessionFor: () => child });
      const status = row("luna-hi").querySelector(".text-\\[12px\\]")?.textContent;
      expect(status).toMatch(/working$/);
    });

    it("shimmers while the open child works on this task, and not on an earlier one", async () => {
      const first = receipt();
      const followUp = receipt({ generation: 2, requestKey: "app-lead-2", task: "Make it longer" });
      const reference = { ...followUp.assignmentReceipt!, task: undefined };
      const child = session({
        id: CHILD,
        busy: true,
        blocks: [
          { id: "u", role: "user", text: "Make it longer", acceptedAssignment: reference as never, startedAt: Date.now() - 42_000 },
        ],
      });
      await render([first, followUp], { sessionFor: (id) => (id === CHILD ? child : undefined) });
      const rows = [...container.querySelectorAll<HTMLElement>("[data-agent-row]")];
      expect(rows).toHaveLength(2);
      expect(rows[0].querySelector(".shimmer-text")).toBeNull();
      expect(rows[0].textContent).not.toContain("working");
      expect(rows[1].querySelector(".shimmer-text")?.textContent).toBe("luna-hi");
      expect(rows[1].textContent).toContain("follow-up");
      expect(rows[1].textContent).toContain("working 42s");
    });

    it("settles to the delivered outcome instead of working", async () => {
      const reference = { ...receipt().assignmentReceipt!, task: undefined };
      const child = session({
        id: CHILD,
        busy: true,
        blocks: [{ id: "u", role: "user", text: "goose", acceptedAssignment: reference as never }],
      });
      await render(
        [receipt(), updateBlock("text", [update("settled", { childId: CHILD, excerpt: "Done" })])],
        { sessionFor: () => child },
      );
      const sent = [...container.querySelectorAll<HTMLElement>("[data-agent-row]")][0];
      expect(sent.querySelector(".shimmer-text")).toBeNull();
      expect(sent.querySelector("[data-agent-status]")?.getAttribute("data-agent-status")).toBe("settled");
      expect(sent.textContent).toContain("replied");
    });

    it("shows an untracked message during an active parent turn without a user bubble", async () => {
      await render(
        [receipt({ kind: "message", generation: undefined, childId: "ancestor", requestKey: "app-lead-1", name: undefined, task: "Task with no result yet" } as never)],
        { busy: true },
      );
      expect(container.querySelector(".user-message-bubble")).toBeNull();
      const opened = await open("ancestor");
      expect(opened.textContent).toContain("Task with no result yet");
      expect(opened.querySelector("p[title]")?.textContent).toMatch(/^Untracked message · /);
    });
  });

  describe("result received", () => {
    it("renders the updates as rows at the head of the resumed turn, outside any user bubble", async () => {
      await render([
        { id: "u1", role: "user", text: "start luna" },
        { id: "a1", role: "assistant", text: "Started." },
        updateBlock(rendered),
        { id: "a2", role: "assistant", text: "Luna replied." },
      ]);
      expect(container.textContent).not.toContain("Session update");
      expect(container.textContent).not.toContain("monocode_session_update");
      expect(container.textContent).not.toContain("sessions.read");
      expect(container.querySelectorAll(".user-message-bubble")).toHaveLength(1);
      const turns = container.querySelectorAll("[data-transcript-turn]");
      const resumed = turns[turns.length - 1];
      const rows = [...resumed.querySelectorAll<HTMLElement>("[data-agent-row]")];
      expect(rows).toHaveLength(3);
      for (const element of rows) expect(element.closest(".user-message-bubble")).toBeNull();
      const reply = [...resumed.querySelectorAll("div")].find((element) => element.textContent === "Luna replied.");
      expect(rows[0].compareDocumentPosition(reply!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(resumed.firstElementChild?.querySelector("[data-session-updates]")).not.toBeNull();
    });

    it("gives each status its word, icon tone and second line", async () => {
      const statuses: SessionUpdateStatus[] = ["settled", "stopped", "failed", "interrupted", "removed", "approval", "question"];
      await render([
        updateBlock("text", statuses.map((status) => update(status, {
          excerpt: status === "approval" || status === "question" ? undefined : `reply ${status}`,
          ...(status === "failed" ? { error: "Provider returned 429" } : {}),
          ...(status === "approval" ? { label: "Write stories/gerald.md" } : {}),
          ...(status === "question" ? { label: "Should Gerald fly south?" } : {}),
        }))),
      ]);
      const expected: Record<SessionUpdateStatus, { word: string; tone: string; second?: string; action?: string }> = {
        settled: { word: "replied", tone: "text-emerald-400" },
        stopped: { word: "stopped by you", tone: "text-content/50" },
        failed: { word: "failed", tone: "text-red-400", second: "Provider returned 429" },
        interrupted: { word: "interrupted", tone: "text-amber-400" },
        removed: { word: "deleted", tone: "text-content/50" },
        approval: { word: "needs your approval", tone: "text-amber-400", second: "Write stories/gerald.md", action: "Review in Agent approval" },
        question: { word: "has a question", tone: "text-amber-400", second: "Should Gerald fly south?", action: "Answer in Agent question" },
      };
      for (const status of statuses) {
        const element = row(`Agent ${status}`);
        const word = element.querySelector<HTMLElement>("[data-agent-status]")!;
        expect(word.getAttribute("data-agent-status")).toBe(status);
        expect(word.textContent).toBe(expected[status].word);
        const toned = word.className.includes(expected[status].tone)
          ? word
          : word.querySelector("svg");
        expect(toned?.getAttribute("class")).toContain(expected[status].tone);
        const second = element.querySelector<HTMLElement>("[data-agent-row-detail]");
        if (expected[status].second) {
          expect(second?.textContent).toContain(expected[status].second);
        } else {
          expect(second).toBeNull();
        }
        if (status === "failed")
          expect(second?.querySelector(".text-red-400")?.textContent).toBe("Provider returned 429");
        if (expected[status].action) {
          await act(async () => button(element, expected[status].action!).click());
          expect(onOpenSession).toHaveBeenLastCalledWith(`child-${status}`);
          expect(element.querySelector("button[aria-expanded]")).toBeNull();
        }
      }
    });

    it("opens a reply with the clamp, Open, Copy reply and the ids line", async () => {
      await render([
        updateBlock("text", [
          update("settled", {
            childId: CHILD,
            title: "Goose story",
            model: "claude:haiku-4.5",
            generation: 3,
            excerpt: "the second rehearsal went worse",
            truncated: true,
            assignment: { kind: "linked", parentId: "lead", childId: CHILD, generation: 3, requestKey: "k", name: "luna-hi" },
          }),
        ]),
      ]);
      const collapsed = row("luna-hi");
      expect(collapsed.textContent).toContain("Haiku 4.5");
      expect(collapsed.textContent).not.toContain("rehearsal");
      expect(collapsed.textContent).not.toContain(CHILD);
      const opened = await open("luna-hi");
      const reply = [...opened.querySelectorAll("p")].find((element) => element.textContent?.includes("rehearsal"));
      expect(reply?.textContent).toBe("…the second rehearsal went worse");
      expect(reply?.className).toContain("line-clamp-4");
      expect(opened.querySelector("p[title]")?.textContent).toMatch(/^Request 3 · claude:haiku-4\.5 · app-/);
      await act(async () => button(opened, "Open luna-hi").click());
      expect(onOpenSession).toHaveBeenCalledWith(CHILD);
      await act(async () => button(opened, "Copy excerpt").click());
      expect(copyMessage).toHaveBeenCalledWith("…the second rehearsal went worse");
    });

    it("offers no way to open a deleted session", async () => {
      await render([updateBlock(rendered)]);
      const opened = await open("Gone");
      expect([...opened.querySelectorAll("button")].some((element) => element.textContent?.startsWith("Open"))).toBe(false);
    });

    it("preserves trailing malformed XML text rather than hiding it", async () => {
      const odd = `${rendered}\nIMPORTANT TRAILING CONTENT`;
      await render([updateBlock(odd)]);
      expect(container.textContent).toContain("IMPORTANT TRAILING CONTENT");
      expect(container.textContent).toContain("monocode_session_update");
    });

    it("keeps showing text that is not a well-formed update as written", async () => {
      await render([updateBlock(rendered.replace('status="settled"', 'status="done"'))]);
      expect(container.textContent).toContain('status="done"');
    });
  });
});
