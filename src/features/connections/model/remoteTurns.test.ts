import { expect, it } from "vitest";
import { remoteTurnUpdates, type RemoteTurnTarget } from "./remoteTurns";

const tab = (shellId: string, busy: boolean, machineId = "mini"): RemoteTurnTarget => ({
  shellId,
  machineId,
  hostSessionId: `host-${shellId}`,
  busy,
});

it("marks a turn that started elsewhere and reloads one that finished", () => {
  const targets = [tab("a", false), tab("b", true), tab("c", true), tab("d", true, "other")];
  expect(
    remoteTurnUpdates(targets, {
      machineId: "mini",
      reset: false,
      sessions: [
        { id: "host-a", projectId: "p", revision: 2, status: "running", busy: true },
        { id: "host-b", projectId: "p", revision: 9, status: "idle", busy: false },
        // Still streaming: nothing to do.
        { id: "host-c", projectId: "p", revision: 4, status: "running", busy: true },
        // Another machine's session with the same host ID is not this tab's.
        { id: "host-d", projectId: "p", revision: 1, status: "idle", busy: false },
      ],
    }),
  ).toEqual({ started: ["a"], finished: [tab("b", true)] });
});

it("ignores writes from hosts that do not report turn state", () => {
  expect(
    remoteTurnUpdates([tab("a", true)], {
      machineId: "mini",
      reset: false,
      sessions: [{ id: "host-a", projectId: "p", revision: 3 }],
    }),
  ).toEqual({ started: [], finished: [] });
});

it("rechecks every busy tab after the host restarts", () => {
  expect(
    remoteTurnUpdates([tab("a", true), tab("b", false)], {
      machineId: "mini",
      reset: true,
      sessions: [],
    }),
  ).toEqual({ started: [], finished: [tab("a", true)] });
});
