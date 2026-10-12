import { beforeEach, expect, it, vi } from "vitest";
import type { AutomationRun } from "../../features/automations/model/automations";
import type { ControlOutcome } from "../../features/orchestration/model/orchestration";
import {
  dispatchInboxAutomationRuns,
  submitAutomationRun,
} from "./automationSubmission";

const update = vi.hoisted(() => vi.fn());
vi.mock("../../features/automations/model/automations", () => ({
  updateAutomationRun: update,
}));
const run = { id: "run", event: "pull_request_head_changed" } as AutomationRun;
beforeEach(() => {
  update.mockReset();
  update.mockResolvedValue({});
});

it("defers a stale claim on release failure and frees it before a newer head claim", async () => {
  let resolveClaim!: (runs: readonly { run: AutomationRun }[]) => void;
  const claim = new Promise<readonly { run: AutomationRun }[]>((resolve) => {
    resolveClaim = resolve;
  });
  let revision = 1;
  const launch = vi.fn(async () => undefined);
  update.mockRejectedValueOnce(new Error("database busy"));
  const dispatch = dispatchInboxAutomationRuns(
    () => claim,
    () => revision === 1,
    launch,
  );

  revision = 2; // A later poll confirmed a draft/closure or a newer head.
  resolveClaim([{ run }]);
  await dispatch;

  expect(update).toHaveBeenCalledExactlyOnceWith(
    "run",
    "pending",
    expect.objectContaining({ error: expect.stringContaining("newer Inbox") }),
  );
  expect(launch).not.toHaveBeenCalled();

  const events: string[] = [];
  const runStates = new Map([["run", "running"]]);
  update.mockImplementation(async (runId: string, status: string) => {
    events.push(`release:${runId}`);
    runStates.set(runId, status);
    return {};
  });
  const newerRun = { ...run, id: "newer" };
  await dispatchInboxAutomationRuns(
    async () => {
      expect(runStates.get("run")).toBe("pending");
      events.push("claim");
      return [{ run: newerRun }];
    },
    () => revision === 2,
    async (item) => {
      events.push(`launch:${item.run.id}`);
    },
  );
  expect(events).toEqual(["release:run", "claim", "launch:newer"]);
});

it.each(["false", "throw", "early failure"])(
  "retains a head claim after %s launch rejection",
  async (mode) => {
    const release = vi.fn();
    await submitAutomationRun(
      run,
      "session",
      (settle) => {
        if (mode === "throw") throw new Error("provider unavailable");
        if (mode === "early failure")
          settle({ status: "failed", text: "", error: "sync failed" });
        return false;
      },
      release,
    );
    expect(update).toHaveBeenCalledExactlyOnceWith(
      "run",
      "pending",
      expect.objectContaining({
        sessionId: "session",
        error: expect.any(String),
      }),
    );
    expect(release).toHaveBeenCalledOnce();
  },
);

it.each(["failed", "completed", "cancelled"] as const)(
  "never retries an accepted turn that later %s",
  async (status) => {
    let settle!: (outcome: ControlOutcome) => void;
    const release = vi.fn();
    await submitAutomationRun(
      run,
      "session",
      (callback) => {
        settle = callback;
        return true;
      },
      release,
    );
    expect(update).not.toHaveBeenCalled();
    expect(release).not.toHaveBeenCalled();
    settle({ status, text: "result" });
    await vi.waitFor(() => expect(release).toHaveBeenCalledOnce());
    expect(update.mock.calls.map(([, state]) => state)).toEqual([
      status === "completed" ? "succeeded" : status,
    ]);
  },
);

it("keeps opened-event launch rejection behavior", async () => {
  await submitAutomationRun(
    { ...run, event: "pull_request_opened" },
    "session",
    () => false,
    vi.fn(),
  );
  expect(update.mock.calls.map(([, state]) => state)).toEqual(["failed"]);
});
