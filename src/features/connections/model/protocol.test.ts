import { expect, it } from "vitest";
import {
  applySessionSync,
  hostSupportsProviderSwitch,
  SESSION_PROVIDER_SWITCH_CAPABILITY,
  type HostDescriptor,
  type HostSession,
} from "./protocol";

const known: HostSession = {
  projectId: "project",
  revision: 4,
  status: "running",
  updatedAt: 0,
  session: {
    id: "session",
    harness: "codex",
    model: "codex:test",
    modelSettings: {},
    runtimeMode: "supervised",
    cwd: "/host/repo",
    title: "Work",
    busy: true,
    blocks: [
      { id: "user", role: "user", text: "Do it" },
      { id: "reply", role: "assistant", text: "Work", streaming: true },
    ],
  },
};

it("applies changed blocks and keeps unchanged ones", () => {
  const next = applySessionSync(known, {
    kind: "delta",
    base: 4,
    value: { ...known, revision: 6, session: { ...known.session, busy: false } },
    blockIds: ["user", "reply", "done"],
    blocks: [
      { id: "reply", role: "assistant", text: "Work done" },
      { id: "done", role: "system", text: "Finished" },
    ],
  });
  expect(next.revision).toBe(6);
  expect(next.session.busy).toBe(false);
  expect(next.session.blocks.map((block) => block.text)).toEqual([
    "Do it",
    "Work done",
    "Finished",
  ]);
  expect(next.session.blocks[0]).toBe(known.session.blocks[0]);
});

it("returns the known value when nothing changed", () => {
  expect(applySessionSync(known, { kind: "unchanged", revision: 4 })).toBe(
    known,
  );
});

it("rejects deltas that do not apply, so the caller loads a snapshot", () => {
  expect(() =>
    applySessionSync(known, { kind: "unchanged", revision: 3 }),
  ).toThrow();
  expect(() =>
    applySessionSync(known, {
      kind: "delta",
      base: 4,
      value: { ...known, revision: 5 },
      blockIds: ["user", "unknown"],
      blocks: [],
    }),
  ).toThrow();
  expect(() =>
    applySessionSync(undefined, { kind: "unchanged", revision: 4 }),
  ).toThrow();
});

it("enables provider switching only when the host advertises it", () => {
  const host: HostDescriptor = {
    protocolVersion: 1,
    environmentId: "host",
    name: "Host",
    providers: ["codex", "claude"],
    capabilities: [],
  };
  expect(hostSupportsProviderSwitch()).toBe(false);
  expect(hostSupportsProviderSwitch(host)).toBe(false);
  expect(hostSupportsProviderSwitch({
    ...host,
    capabilities: [SESSION_PROVIDER_SWITCH_CAPABILITY],
  })).toBe(true);
});
