import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  acquireHarnessBridge: vi.fn(),
  close: vi.fn(),
  frames: [] as Array<(record: Record<string, unknown>) => void>,
  killChild: vi.fn(),
  onExit: undefined as ((code: number | null) => void) | undefined,
  pushLine: vi.fn(),
  releaseBridge: vi.fn(),
  request: vi.fn(),
  resolveBinary: vi.fn(),
  spawnChild: vi.fn(),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  writeChild: vi.fn(),
}));

vi.mock("./child", () => ({
  acquireHarnessBridge: mocks.acquireHarnessBridge,
  killChild: mocks.killChild,
  resolveOmpBinary: vi.fn(),
  resolvePiBinary: mocks.resolveBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: mocks.unwatchChild,
  watchChild: mocks.watchChild,
  writeChild: mocks.writeChild,
}));

vi.mock("./piClient", () => ({
  PiRpc: class {
    pushLine = mocks.pushLine;
    request = mocks.request;
    close = mocks.close;

    constructor(
      _sessionId: string,
      onFrame: (record: Record<string, unknown>) => void,
    ) {
      mocks.frames.push(onFrame);
    }
  },
}));

import {
  discoverPiSkills,
  piSkillsFromRpcData,
} from "./piSkills";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

beforeEach(() => {
  for (const value of Object.values(mocks)) {
    if (typeof value === "function" && "mockReset" in value) {
      value.mockReset();
    }
  }
  mocks.frames.length = 0;
  mocks.onExit = undefined;
  mocks.acquireHarnessBridge.mockResolvedValue(mocks.releaseBridge);
  mocks.resolveBinary.mockResolvedValue({ path: "/bin/pi" });
  mocks.spawnChild.mockResolvedValue(undefined);
  mocks.killChild.mockResolvedValue(undefined);
  mocks.writeChild.mockResolvedValue(undefined);
  mocks.request.mockResolvedValue({ data: { commands: [] } });
  mocks.watchChild.mockImplementation(
    (
      _id: string,
      _onLine: (line: string) => void,
      onExit: (code: number | null) => void,
    ) => {
      mocks.onExit = onExit;
    },
  );
});

describe("piSkillsFromRpcData", () => {
  it("keeps the first valid row for each Pi skill invocation", () => {
    expect(
      piSkillsFromRpcData({
        commands: [
          { name: "help", description: "Help", source: "builtin" },
          {
            name: "skill:architect",
            description: "Design before implementation.",
            source: "skill",
            sourceInfo: { path: "/tmp/architect/SKILL.md" },
          },
          {
            name: "skill:architect",
            description: "Duplicate",
            source: "skill",
          },
          { name: "skill:", source: "skill" },
          { name: 42, source: "skill" },
        ],
      }),
    ).toEqual([
      {
        name: "architect",
        description: "Design before implementation.",
        invocation: "skill:architect",
        source: "pi",
      },
    ]);
  });

  it.each([undefined, null, {}, { commands: null }])(
    "rejects a malformed commands envelope: %j",
    (data) => expect(() => piSkillsFromRpcData(data)).toThrow(/commands/),
  );
});

describe("discoverPiSkills", () => {
  it("loads skills through an isolated sessionless Pi probe", async () => {
    mocks.request.mockResolvedValue({
      data: {
        commands: [
          {
            name: "skill:architect",
            description: "Design first.",
            source: "skill",
          },
        ],
      },
    });

    await expect(discoverPiSkills("/repo")).resolves.toEqual([
      {
        name: "architect",
        description: "Design first.",
        invocation: "skill:architect",
        source: "pi",
      },
    ]);

    expect(mocks.acquireHarnessBridge).toHaveBeenCalledOnce();
    const [childId, command, args, cwd] = mocks.spawnChild.mock.calls[0]!;
    expect(childId).toMatch(/^monocode-pi-skills-/);
    expect(command).toBe("/bin/pi");
    expect(args).toEqual(["--mode", "rpc", "--no-session"]);
    expect(cwd).toBe("/repo");
    expect(mocks.request).toHaveBeenCalledWith(
      { type: "get_commands" },
      45_000,
    );
    expect(mocks.close).toHaveBeenCalledOnce();
    expect(mocks.unwatchChild).toHaveBeenCalledWith(childId);
    expect(mocks.killChild).toHaveBeenCalledWith(childId);
    expect(mocks.releaseBridge).toHaveBeenCalledOnce();
  });

  it("denies extension UI requests that require a reply", async () => {
    const response = deferred<Record<string, unknown>>();
    mocks.request.mockReturnValue(response.promise);
    const discovery = discoverPiSkills("/repo");
    await vi.waitFor(() => expect(mocks.frames).toHaveLength(1));
    const childId = mocks.spawnChild.mock.calls[0]![0] as string;

    mocks.frames[0]!({
      type: "extension_ui_request",
      id: "ui-1",
      method: "confirm",
      title: "Continue?",
    });
    await vi.waitFor(() => expect(mocks.writeChild).toHaveBeenCalledOnce());
    expect(mocks.writeChild).toHaveBeenCalledWith(
      childId,
      JSON.stringify({
        type: "extension_ui_response",
        id: "ui-1",
        cancelled: true,
      }),
    );

    response.resolve({ data: { commands: [] } });
    await discovery;
  });

  it("cleans up after request and response failures", async () => {
    mocks.request.mockRejectedValueOnce(new Error("rpc failed"));
    await expect(discoverPiSkills("/repo")).rejects.toThrow("rpc failed");
    expect(mocks.killChild).toHaveBeenCalledOnce();
    expect(mocks.releaseBridge).toHaveBeenCalledOnce();

    mocks.request.mockResolvedValueOnce({ data: {} });
    await expect(discoverPiSkills("/repo")).rejects.toThrow(/commands/);
    expect(mocks.killChild).toHaveBeenCalledTimes(2);
    expect(mocks.releaseBridge).toHaveBeenCalledTimes(2);
  });

  it("uses a unique child id for each probe", async () => {
    await discoverPiSkills("/repo");
    await discoverPiSkills("/repo");
    expect(mocks.spawnChild.mock.calls[0]![0]).not.toBe(
      mocks.spawnChild.mock.calls[1]![0],
    );
  });
});
