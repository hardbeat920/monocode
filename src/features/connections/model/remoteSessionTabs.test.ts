// @vitest-environment happy-dom
import { beforeEach, expect, it } from "vitest";
import { newTab, splitPane } from "../../workspace/model/layout";
import { rememberRemoteSession } from "./connections";
import { findRemoteSessionTab } from "./remoteSessionTabs";

const project = "remote://host/Users/me/code/monocode";
const sessionId = "host-session";

beforeEach(() => localStorage.clear());

it.each(["monocode", "monocode-local"])(
  "opens the remote tab instead of a local %s tab with a stale binding",
  (name) => {
    const local = { id: "local-shell", cwd: `/Users/me/code/${name}` };
    const remote = { id: "remote-shell", cwd: project };
    const localTab = newTab(local.id);
    const remoteTab = newTab(remote.id);
    rememberRemoteSession(local.id, sessionId);
    rememberRemoteSession(remote.id, sessionId);

    expect(
      findRemoteSessionTab(
        [localTab, remoteTab],
        [local, remote],
        project,
        sessionId,
      ),
    ).toEqual({ tab: remoteTab, shellId: remote.id });
    expect(local.cwd).toBe(`/Users/me/code/${name}`);
  },
);

it("opens a new remote tab when the only binding belongs to a local tab", () => {
  const local = { id: "local-shell", cwd: "/Users/me/code/monocode" };
  rememberRemoteSession(local.id, sessionId);

  expect(
    findRemoteSessionTab([newTab(local.id)], [local], project, sessionId),
  ).toBeUndefined();
});

it.each([
  "remote://other-host/Users/me/code/monocode",
  "remote://host/Users/me/other/monocode",
])("does not select the matching session ID from %s", (otherProject) => {
  const other = { id: "other-shell", cwd: otherProject };
  rememberRemoteSession(other.id, sessionId);

  expect(
    findRemoteSessionTab([newTab(other.id)], [other], project, sessionId),
  ).toBeUndefined();
});

it("selects the remote pane in a mixed-project split tab", () => {
  const local = { id: "local-shell", cwd: "/Users/me/code/monocode" };
  const remote = { id: "remote-shell", cwd: project };
  const tab = newTab(local.id);
  tab.layout = splitPane(tab.layout, local.id, "right", remote.id);
  rememberRemoteSession(local.id, sessionId);
  rememberRemoteSession(remote.id, sessionId);

  expect(
    findRemoteSessionTab([tab], [local, remote], project, sessionId),
  ).toEqual({ tab, shellId: remote.id });
});

it("ignores a binding whose shell is no longer mounted", () => {
  const tab = newTab("missing-shell");
  rememberRemoteSession("missing-shell", sessionId);

  expect(findRemoteSessionTab([tab], [], project, sessionId)).toBeUndefined();
});

it("reuses the existing tab when the remote project has a trailing slash", () => {
  const remote = { id: "remote-shell", cwd: project };
  const tab = newTab(remote.id);
  rememberRemoteSession(remote.id, sessionId);

  expect(
    findRemoteSessionTab([tab], [remote], `${project}/`, sessionId),
  ).toEqual({ tab, shellId: remote.id });
});
