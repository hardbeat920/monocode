import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PtyCommand } from "../../../../platform/tauri/pty";

const sent: string[] = [];
const spawnedHeadless: string[][] = [];
let onLine: ((line: string) => void) | undefined;

vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawnedHeadless.push(args);
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const {
  closeRemoteControl,
  openRemoteControl,
  remoteControlPtyId,
  REMOTE_CONTROL_COLS,
  REMOTE_CONTROL_ROWS,
} = await import("./remoteControl");
const { bindClaudeSession, sendClaudeTurn, stopClaudeSession, __claudeTestReset } =
  await import("./claude");

/** Every port records into one log, so ordering is asserted rather than implied. */
type Recorder = {
  calls: string[];
  ptySpawns: {
    id: string;
    cwd: string;
    cols: number;
    rows: number;
    command: PtyCommand;
  }[];
  killed: string[];
};

function recorder(
  overrides: {
    transcriptEnd?: () => Promise<number>;
    spawnPty?: () => Promise<void>;
  } = {},
) {
  const rec: Recorder = { calls: [], ptySpawns: [], killed: [] };
  const ports = {
    stopSession: async (sessionId: string) => {
      rec.calls.push(`stop:${sessionId}`);
      await stopClaudeSession(sessionId);
    },
    transcriptEnd: async () => {
      rec.calls.push("transcriptEnd");
      if (overrides.transcriptEnd) return overrides.transcriptEnd();
      return 4096;
    },
    spawnPty: async (
      id: string,
      cwd: string,
      cols: number,
      rows: number,
      command: PtyCommand,
    ) => {
      rec.calls.push(`spawnPty:${id}`);
      rec.ptySpawns.push({ id, cwd, cols, rows, command });
      if (overrides.spawnPty) await overrides.spawnPty();
    },
    killPty: async (id: string) => {
      rec.calls.push(`killPty:${id}`);
      rec.killed.push(id);
    },
  };
  return { rec, ports };
}

const target = {
  sessionId: "s1",
  providerSessionId: "sess-abc",
  cwd: "/repo/worktrees/feature",
  name: "monocode",
};

const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 200; i++) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
};

/** Runs a headless turn to completion so the resume binding can be observed. */
async function headlessTurn() {
  const turn = sendClaudeTurn({
    sessionId: "s1",
    cwd: "/repo/worktrees/feature",
    model: "claude:claude-sonnet-5",
    modelSettings: {},
    runtimeMode: "supervised",
    text: "carry on",
    attachments: [],
    onEvent: () => undefined,
  });
  await waitFor(() => spawnedHeadless.length > 0, "headless spawn");
  onLine!(JSON.stringify({ type: "system", subtype: "init", session_id: "sess-abc" }));
  await waitFor(
    () => sent.some((line) => (JSON.parse(line) as { type?: string }).type === "user"),
    "user prompt",
  );
  onLine!(JSON.stringify({ type: "result", subtype: "success", session_id: "sess-abc" }));
  await turn;
}

beforeEach(() => {
  sent.length = 0;
  spawnedHeadless.length = 0;
  onLine = undefined;
  __claudeTestReset();
});

afterEach(async () => {
  await stopClaudeSession("s1");
  __claudeTestReset();
});

describe("claude remote control handover", () => {
  it("stops the headless child before the pty starts", async () => {
    const { rec, ports } = recorder();

    const handle = await openRemoteControl(target, ports);

    // Two processes appending to one transcript interleave their records, so
    // this order is the feature rather than a detail of it. The transcript is
    // measured in between, while neither side can be writing to it.
    expect(rec.calls).toEqual(["stop:s1", "transcriptEnd", "spawnPty:remote-control:s1"]);
    expect(handle).toEqual({ ptyId: "remote-control:s1", offset: 4096 });
  });

  it("resumes the bound conversation in the working copy, not the project root", async () => {
    const { rec, ports } = recorder();

    await openRemoteControl(target, ports);

    expect(rec.ptySpawns).toEqual([
      {
        id: "remote-control:s1",
        cwd: "/repo/worktrees/feature",
        cols: REMOTE_CONTROL_COLS,
        rows: REMOTE_CONTROL_ROWS,
        command: {
          program: "claude",
          args: ["--resume", "sess-abc", "--remote-control", "monocode"],
        },
      },
    ]);
  });

  it("takes a configured binary path instead of the bare name", async () => {
    const { rec, ports } = recorder();

    await openRemoteControl({ ...target, program: "/opt/homebrew/bin/claude" }, ports);

    expect(rec.ptySpawns[0]?.command.program).toBe("/opt/homebrew/bin/claude");
  });

  it("refuses a thread with no bound session id without stopping anything", async () => {
    const { rec, ports } = recorder();

    await expect(
      openRemoteControl({ ...target, providerSessionId: undefined }, ports),
    ).rejects.toThrow(/nothing to hand over/);

    // Refusing has to leave a working session working: stopping the child to
    // run a `--resume` with nothing to resume loses the session for nothing.
    expect(rec.calls).toEqual([]);
  });

  it("kills the pty when the conversation is taken back", async () => {
    const { rec, ports } = recorder();
    await openRemoteControl(target, ports);
    rec.calls.length = 0;

    await closeRemoteControl("s1", ports);

    expect(rec.killed).toEqual([remoteControlPtyId("s1")]);
  });

  it("keeps the resume binding across the round trip", async () => {
    bindClaudeSession("s1", "sess-abc", "/repo/worktrees/feature");
    const { ports } = recorder();

    await openRemoteControl(target, ports);
    await closeRemoteControl("s1", ports);
    await headlessTurn();

    // stopClaudeSession preserves resumeByThread, so the conversation the pty
    // was holding is the one the next headless turn continues.
    expect(spawnedHeadless).toHaveLength(1);
    expect(spawnedHeadless[0]).toEqual(
      expect.arrayContaining(["--resume", "sess-abc"]),
    );
  });

  describe("a failure part way through", () => {
    it("reaps a pty that failed to start", async () => {
      const { rec, ports } = recorder({
        spawnPty: async () => {
          throw new Error("claude: command not found");
        },
      });

      await expect(openRemoteControl(target, ports)).rejects.toThrow(
        "claude: command not found",
      );

      // A pty that got far enough to hold the conversation would lock out the
      // headless child that takes over on the next turn.
      expect(rec.killed).toEqual([remoteControlPtyId("s1")]);
    });

    it("does not spawn when the transcript cannot be measured", async () => {
      const { rec, ports } = recorder({
        transcriptEnd: async () => {
          throw new Error("EACCES");
        },
      });

      await expect(openRemoteControl(target, ports)).rejects.toThrow("EACCES");

      // Without the boundary the mirror would replay the whole conversation as
      // if it were new, so the pty must not open at all.
      expect(rec.calls).toEqual(["stop:s1", "transcriptEnd"]);
      expect(rec.ptySpawns).toEqual([]);
    });

    it("leaves the thread able to carry on headless", async () => {
      bindClaudeSession("s1", "sess-abc", "/repo/worktrees/feature");
      const { ports } = recorder({
        spawnPty: async () => {
          throw new Error("claude: command not found");
        },
      });

      await expect(openRemoteControl(target, ports)).rejects.toThrow();

      // Recovery is that there is nothing to recover: no process is the state
      // every thread sits in between turns, and the binding is untouched, so
      // the next turn resumes the same conversation.
      await headlessTurn();
      expect(spawnedHeadless[0]).toEqual(
        expect.arrayContaining(["--resume", "sess-abc"]),
      );
    });
  });
});
