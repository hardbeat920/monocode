// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionUpdate } from "../../agent-app/model/sessionLinks";
import type { Block, Session } from "../model/session";
import { AgentTranscript, type SessionPeer } from "./AgentTranscript";

const accepted = {
  kind: "linked" as const,
  parentId: "lead",
  childId: "child",
  generation: 2,
  requestKey: "app-lead-4",
};

const task: Block = {
  id: "task",
  role: "user",
  text: "Write a much bigger version of the goose story",
  acceptedAssignment: accepted,
  startedAt: 1_000,
  durationMs: 2_000,
};
const reply: Block = { id: "reply", role: "assistant", text: "Gerald the goose" };

function parent(blocks: Block[] = []): Session {
  return {
    id: "lead",
    harness: "claude",
    model: "claude:haiku-4.5",
    modelSettings: {},
    runtimeMode: "supervised",
    title: "main",
    cwd: "/repo",
    blocks,
  };
}

function delivered(generation: number): Block {
  const update: SessionUpdate = {
    id: `child:${generation}:outcome`,
    kind: "outcome",
    status: "settled",
    parentId: "lead",
    childId: "child",
    generation,
    title: "Goose",
    harness: "pi",
    excerpt: "Gerald the goose",
    at: 1,
  };
  return { id: `update-${generation}`, role: "user", text: "", sessionUpdate: { deliveryId: "d", updates: [update] } };
}

describe("a task received from another session", () => {
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
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  async function render(blocks: Block[], lead?: SessionPeer) {
    await act(async () =>
      root.render(
        createElement(AgentTranscript, {
          blocks,
          busy: false,
          onOpenSession,
          sessionFor: (id: string) => (id === lead?.id ? lead : undefined),
        }),
      ),
    );
  }

  const bubble = () => container.querySelector<HTMLElement>(".user-message-bubble");
  const footer = () => container.querySelector<HTMLElement>('[aria-label="Worked for 2s"]');

  it("names the parent's model and title over a tinted bubble, and opens the parent", async () => {
    await render([task, reply], parent());
    expect(bubble()?.dataset.fromParent).toBe("true");
    expect(bubble()?.textContent).toContain("Task fromHaiku 4.5 · main");
    const link = bubble()?.querySelector("button");
    expect(link?.textContent).toBe("Haiku 4.5 · main");
    await act(async () => link?.click());
    expect(onOpenSession).toHaveBeenCalledWith("lead");
  });

  it("names a parent known only from history and still opens it, without claiming delivery", async () => {
    const { blocks: _blocks, ...summary } = parent([delivered(2)]);
    await render([task, reply], summary);
    expect(bubble()?.textContent).toContain("Task fromHaiku 4.5 · main");
    expect(bubble()?.textContent).not.toContain("lead");
    await act(async () => bubble()?.querySelector("button")?.click());
    expect(onOpenSession).toHaveBeenCalledWith("lead");
    expect(footer()?.textContent).not.toContain("Sent to");
  });

  it("drops the model from the sender line when the parent has none", async () => {
    await render([task, reply], { ...parent(), harness: "codex", model: "" });
    expect(bubble()?.querySelector("button")?.textContent).toBe("main");
  });

  it("falls back to the parent id with no link when the app does not know the parent", async () => {
    await render([task, reply]);
    expect(bubble()?.textContent).toContain("Task fromlead");
    expect(bubble()?.querySelector("button")).toBeNull();
    expect(footer()?.textContent).not.toContain("Sent to");
  });

  it("adds Sent to the parent after the clock only once this generation's reply was delivered", async () => {
    await render([task, reply], parent());
    expect(footer()?.textContent).not.toContain("Sent to");

    await render([task, reply], parent([delivered(1)]));
    expect(footer()?.textContent).not.toContain("Sent to");

    await render([task, reply], parent([delivered(2)]));
    const sent = [...(footer()?.querySelectorAll("button") ?? [])].find((element) => element.textContent === "Sent to main");
    expect(sent).toBeDefined();
    const clock = footer()?.querySelector(".text-content\\/35");
    expect(clock && sent && clock.compareDocumentPosition(sent) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await act(async () => sent?.click());
    expect(onOpenSession).toHaveBeenCalledWith("lead");
  });

  it("leaves a prompt the user typed alone", async () => {
    await render([{ ...task, acceptedAssignment: undefined }, reply], parent([delivered(2)]));
    expect(bubble()?.dataset.fromParent).toBeUndefined();
    expect(container.textContent).not.toContain("Task from");
    expect(container.textContent).not.toContain("Sent to");
  });
});
