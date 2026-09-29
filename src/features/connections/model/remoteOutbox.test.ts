// @vitest-environment happy-dom
import { beforeEach, expect, it } from "vitest";
import {
  pendingRemoteCommand,
  pendingRemoteFollowup,
  savePendingRemoteCommand,
} from "./connections";
import type { HostCommand } from "./protocol";

beforeEach(() => localStorage.clear());
const create: HostCommand = {
  type: "create",
  commandId: "create-1",
  projectId: "p",
  harness: "codex",
  model: "test",
  runtimeMode: "supervised",
};
const followup: HostCommand = {
  type: "send",
  commandId: "send-1",
  sessionId: "",
  text: "First message",
};

it("isolates unfinished creates by tab and keeps their original first message on retry", () => {
  savePendingRemoteCommand("project", "env", create, "first-tab", followup);
  expect(
    pendingRemoteCommand("project", "env", null, "second-tab"),
  ).toBeUndefined();
  expect(pendingRemoteCommand("project", "env", null, "first-tab")).toEqual(
    create,
  );
  savePendingRemoteCommand("project", "env", create, "first-tab");
  expect(pendingRemoteFollowup("project", "env", create.commandId)).toEqual(
    followup,
  );
});

it("can recover commands saved by the earlier desktop", () => {
  localStorage.setItem(
    'monocode.remote-command.v1:["project","env"]:create-1',
    JSON.stringify(create),
  );
  expect(pendingRemoteCommand("project", "env", null, "first-tab")).toEqual(
    create,
  );
});
