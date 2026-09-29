// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  REMOTE_CHANGES,
  remoteChangesLive,
  watchRemoteChanges,
  type RemoteChangesDetail,
} from "./connections";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => vi.mocked(invoke).mockReset());

it("holds one wait per machine and reports each batch of session writes", async () => {
  const answers = [
    { boot: "b", cursor: 4, sessions: [], reset: true },
    {
      boot: "b",
      cursor: 6,
      sessions: [{ id: "s1", projectId: "p", revision: 3 }],
      reset: false,
    },
    { boot: "c", cursor: 0, sessions: [], reset: true },
  ];
  const waits: unknown[] = [];
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    waits.push((args as { params: unknown }).params);
    return answers.shift() ?? new Promise(() => {});
  });
  const events: RemoteChangesDetail[] = [];
  const listener = (event: Event) =>
    events.push((event as CustomEvent<RemoteChangesDetail>).detail);
  window.addEventListener(REMOTE_CHANGES, listener);
  const stopFirst = watchRemoteChanges("machine");
  const stopSecond = watchRemoteChanges("machine");
  await vi.waitFor(() => expect(waits).toHaveLength(4));
  // The first answer only sets the cursor; the host restart is a reset.
  expect(events).toEqual([
    {
      machineId: "machine",
      sessions: [{ id: "s1", projectId: "p", revision: 3 }],
      reset: false,
    },
    { machineId: "machine", sessions: [], reset: true },
  ]);
  expect(waits.slice(0, 3)).toEqual([
    { boot: undefined, after: 0 },
    { boot: "b", after: 4 },
    { boot: "b", after: 6 },
  ]);
  expect(remoteChangesLive("machine")).toBe(true);
  stopFirst();
  expect(remoteChangesLive("machine")).toBe(true);
  stopSecond();
  expect(remoteChangesLive("machine")).toBe(false);
  window.removeEventListener(REMOTE_CHANGES, listener);
});

it("stops waiting on a host without pushed changes", async () => {
  vi.mocked(invoke).mockRejectedValue(
    "Host rejected request: Unsupported host method",
  );
  const stop = watchRemoteChanges("old-host");
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(remoteChangesLive("old-host")).toBe(false);
  stop();
});
