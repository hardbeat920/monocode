import { afterEach, expect, it, vi } from "vitest";
import {
  buildRemotePlan,
  registerRemoteSessionActions,
} from "./remoteSessionActions";

type RemoteActions = Parameters<typeof registerRemoteSessionActions>[1];

let unregister = () => {};
afterEach(() => {
  unregister();
  unregister = () => {};
});

function remoteActions(buildPlan: RemoteActions["buildPlan"]): RemoteActions {
  return {
    buildPlan,
    submit: vi.fn(),
    saveDraft: vi.fn(),
    stop: vi.fn(),
    compact: vi.fn(() => true),
    approve: vi.fn(),
    answer: vi.fn(),
  };
}

it("returns the acceptance from a registered remote Build handler", () => {
  const buildPlan = vi.fn(() => true);
  unregister = registerRemoteSessionActions("shell", remoteActions(buildPlan));

  expect(buildRemotePlan("shell", "plan")).toBe(true);
  expect(buildPlan).toHaveBeenCalledWith("plan", undefined);
});

it("does not treat a missing remote Build handler as accepted", () => {
  expect(buildRemotePlan("missing", "plan")).toBeUndefined();
});
