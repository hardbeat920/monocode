import { describe, expect, it, vi } from "vitest";
import { stopSessionProcesses } from "./stopSessionProcesses";

describe("stopSessionProcesses", () => {
  it("finishes the control turn, then reports a kill that failed", async () => {
    const order: string[] = [];
    const killError = new Error("The agent's process did not exit");
    const stopping = stopSessionProcesses({
      stopChildren: async () => void order.push("children"),
      kill: async () => {
        order.push("kill");
        throw killError;
      },
      finishTurn: async () => void order.push("finish"),
    });
    await expect(stopping).rejects.toBe(killError);
    expect(order).toEqual(["children", "kill", "finish"]);
  });

  it("resolves once the process is gone", async () => {
    const finishTurn = vi.fn(async () => undefined);
    await expect(
      stopSessionProcesses({
        stopChildren: async () => undefined,
        kill: async () => undefined,
        finishTurn,
      }),
    ).resolves.toBeUndefined();
    expect(finishTurn).toHaveBeenCalledOnce();
  });

  it("still kills and finishes the turn when stopping a child fails", async () => {
    const childError = new Error("child stop failed");
    const kill = vi.fn(async () => {
      throw new Error("kill failed");
    });
    const finishTurn = vi.fn(async () => undefined);
    await expect(
      stopSessionProcesses({
        stopChildren: async () => {
          throw childError;
        },
        kill,
        finishTurn,
      }),
    ).rejects.toBe(childError);
    expect(kill).toHaveBeenCalledOnce();
    expect(finishTurn).toHaveBeenCalledOnce();
  });

  it.each([
    [["stopChildren", "kill", "finishTurn"], "stopChildren"],
    [["kill", "finishTurn"], "kill"],
    [["stopChildren", "finishTurn"], "stopChildren"],
    [["finishTurn"], "finishTurn"],
  ] as const)(
    "runs every step when %j fail and reports %s",
    async (failing, reported) => {
      const calls: string[] = [];
      const step = (name: string) => async () => {
        calls.push(name);
        if ((failing as readonly string[]).includes(name))
          throw new Error(name);
      };
      await expect(
        stopSessionProcesses({
          stopChildren: step("stopChildren"),
          kill: step("kill"),
          finishTurn: step("finishTurn"),
        }),
      ).rejects.toThrow(new Error(reported));
      expect(calls).toEqual(["stopChildren", "kill", "finishTurn"]);
    },
  );
});
