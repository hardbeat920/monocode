import { describe, expect, it } from "vitest";
import type { Register, WorkerToHost } from "../api/types";
import { createRuntime, jsx, Box, Button, Text } from "./worker";

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

function setup(register: Register) {
  const sent: WorkerToHost[] = [];
  const runtime = createRuntime((m) => sent.push(m));
  const loaded = runtime.load(register, { trunk: "main" });
  const results = () => sent.filter((m) => m.kind === "result");
  return { runtime, sent, loaded, results };
}

describe("worker runtime", () => {
  it("runs handlers in order and lets next pass a rewritten event", async () => {
    const seen: string[] = [];
    const { runtime, sent, loaded, results } = setup((on) => {
      on("command.run", { command: "pr" }, async (_$, e, next) => {
        seen.push(`a:${e.args}`);
        return next({ ...e, args: "changed" });
      });
      on("command.run", { command: "pr" }, (_$, e) => {
        seen.push(`b:${e.args}`);
        return { text: "done" };
      });
      on("command.run", { command: "pr" }, () => {
        seen.push("unreached");
      });
      on("command.run", { command: "other" }, () => {
        seen.push("filtered");
      });
    });
    await loaded;
    expect(sent[0]).toMatchObject({ kind: "ready" });

    runtime.receive({
      kind: "event",
      id: 1,
      name: "command.run",
      event: { command: "pr", args: "x" },
    });
    await tick();
    expect(seen).toEqual(["a:x", "b:changed"]);
    expect(results()).toEqual([
      { kind: "result", id: 1, ok: true, value: { text: "done" } },
    ]);
  });

  it("resolves tool.call next when the host reports the call done", async () => {
    let outcome: unknown;
    const { runtime, loaded } = setup((on) => {
      on("tool.call", { tool: "Bash" }, async (_$, e, next) => {
        outcome = await next(e);
      });
    });
    await loaded;
    runtime.receive({
      kind: "event",
      id: 1,
      name: "tool.call",
      event: { sessionId: "s", callId: "c1", tool: "Bash" },
    });
    await tick();
    expect(outcome).toBeUndefined();
    runtime.receive({
      kind: "tool.done",
      callId: "c1",
      result: { isError: false },
    });
    await tick();
    expect(outcome).toEqual({ isError: false });
  });

  it("keeps a tool.done that arrives while the handler awaits before next", async () => {
    let outcome: unknown;
    const { runtime, sent, loaded } = setup((on, $) => {
      on("tool.call", null, async (_$, e, next) => {
        await $.store.get("k");
        outcome = await next(e);
      });
    });
    await loaded;
    runtime.receive({
      kind: "event",
      id: 1,
      name: "tool.call",
      event: { sessionId: "s", callId: "c1", tool: "Read" },
    });
    runtime.receive({
      kind: "tool.done",
      callId: "c1",
      result: { isError: false },
    });
    const call = sent.find((m) => m.kind === "call");
    if (call?.kind !== "call") throw new Error("no store call");
    runtime.receive({ kind: "result", id: call.id, ok: true, value: null });
    await tick();
    expect(outcome).toEqual({ isError: false });
  });

  it("sends trees with callbacks as ids and invalidates on atom writes", async () => {
    let presses = 0;
    const { runtime, sent, loaded, results } = setup((on, $) => {
      const count = $.state.atom("count", 0);
      on(
        "ui.render",
        { component: "SidebarTab" },
        ($) =>
          jsx(Box, {
            children: [
              jsx(Text, { children: `n=${$.state.read(count)}` }),
              jsx(Button, {
                label: "+",
                onPress: () => {
                  presses++;
                  $.state.update(count, (n) => n + 1);
                },
              }),
            ],
          }) as never,
      );
    });
    await loaded;
    const render = (id: number) =>
      runtime.receive({
        kind: "event",
        id,
        name: "ui.render",
        event: {
          component: "SidebarTab",
          requestId: "v",
          surface: "desktop",
          props: {},
        },
      });
    render(1);
    await tick();
    const tree = results()[0] as {
      value: { children: [unknown, { props: { onPress: { $cb: string } } }] };
    };
    expect(tree.value.children[0]).toEqual({
      type: "Text",
      props: {},
      children: ["n=0"],
    });
    const cb = tree.value.children[1].props.onPress.$cb;

    runtime.receive({ kind: "callback", cb, args: [] });
    await tick();
    expect(presses).toBe(1);
    expect(sent).toContainEqual({
      kind: "effect",
      method: "ui.invalidate",
      args: [],
    });

    render(2);
    await tick();
    expect(
      (results()[1] as { value: { children: unknown[] } }).value.children[0],
    ).toMatchObject({ children: ["n=1"] });
    // The earlier render's callbacks are released.
    runtime.receive({ kind: "callback", cb, args: [] });
    expect(presses).toBe(1);
  });

  it("proxies $ calls and passes the event's session to session.current", async () => {
    let current: unknown;
    const { runtime, sent, loaded } = setup((on) => {
      on("turn.start", null, async ($) => {
        current = await $.session.current();
      });
    });
    await loaded;
    runtime.receive({
      kind: "event",
      id: 1,
      name: "turn.start",
      event: { sessionId: "s9" },
    });
    await tick();
    const call = sent.find((m) => m.kind === "call");
    expect(call).toMatchObject({ method: "session.current", args: ["s9"] });
    runtime.receive({
      kind: "result",
      id: (call as { id: number }).id,
      ok: true,
      value: { id: "s9" },
    });
    await tick();
    expect(current).toEqual({ id: "s9" });
  });
});
