import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "./types";
import type { CodexAccount } from "./codexAccounts";
const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  spawn: vi.fn(),
  kill: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
let onLine: (line: string) => void;
let generation = 0;
let loggedIn = "external";
let resumeFails = false;
let disableHot = false;
const mismatchAccounts = new Set<string>();
const badCreds = new Set<string>();
let shared = false;
let rejectStart = false;
let allExhausted = false;
const messages: Array<{
  generation: number;
  method?: string;
  id?: number;
  params?: Record<string, unknown>;
}> = [];
const events: HarnessEvent[] = [];
let pool: CodexAccount[];
const account = (id: string): CodexAccount => ({
  id,
  accountId: id,
  email: `${id}@example.invalid`,
  addedAt: 1,
  hasTokens: true,
});
const emit = (message: unknown) => onLine(JSON.stringify(message));
vi.mock("./child", () => ({
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
  spawnChild: async () => {
    generation++;
    loggedIn = "external";
    await mocks.spawn();
  },
  killChild: async () => {
    mocks.kill();
  },
  unwatchChild: () => {},
  watchChild: (_: string, handler: (line: string) => void) => {
    onLine = handler;
  },
  writeChild: async (_: string, line: string) => {
    const msg = JSON.parse(line);
    messages.push({ generation, ...msg });
    if (!msg.method || msg.id == null) return;
    const reply = (result: unknown) => emit({ id: msg.id, result });
    if (msg.method === "initialize") reply({});
    else if (msg.method === "account/login/start") {
      if (disableHot && generation === 1) {
        emit({ id: msg.id, error: { message: "hot login unsupported" } });
        return;
      }
      loggedIn = msg.params.chatgptAccountId;
      reply({ type: "chatgptAuthTokens" });
    } else if (msg.method === "account/read")
      reply({
        account: {
          email: mismatchAccounts.has(loggedIn)
            ? "wrong@example.invalid"
            : `${loggedIn}@example.invalid`,
        },
      });
    else if (msg.method === "thread/resume" && resumeFails)
      emit({ id: msg.id, error: { message: "resume failed" } });
    else if (msg.method === "thread/start" || msg.method === "thread/resume")
      reply({ thread: { id: "thread-one" } });
    else if (msg.method === "turn/start") {
      const wall = (generation === 1 && loggedIn === "external") || allExhausted;
      emit({
        method: "account/rateLimits/updated",
        params: {
          rateLimits: {
            primary: {
              usedPercent: 100,
              resetsAt: Math.floor(Date.now() / 1000) + 7200,
            },
            rateLimitReachedType: shared
              ? "workspace_owner_credits_depleted"
              : "rate_limit_reached",
          },
        },
      });
      if (rejectStart && wall) {
        emit({ id: msg.id, error: { message: "HTTP 429 quota" } });
        return;
      }
      reply({ turn: { id: `turn-${generation}` } });
      emit({
        method: "turn/completed",
        params: {
          turn: {
            id: `turn-${generation}`,
            status: wall ? "failed" : "completed",
            ...(wall
              ? {
                  error: {
                    message: "Quota exhausted",
                    codexErrorInfo: "usageLimitExceeded",
                  },
                }
              : {}),
          },
        },
      });
    }
  },
}));
const { sendCodexTurn, cancelCodexTurn, stopCodexSession, __codexTestReset } =
  await import("./codex");
const input = (): SendTurnInput => ({
  sessionId: "failover",
  cwd: "/fake/repo",
  model: "codex:gpt-5.4",
  runtimeMode: "supervised",
  text: "original request",
  attachments: [],
  onEvent: (event) => events.push(event),
});
beforeEach(() => {
  generation = 0;
  loggedIn = "external";
  resumeFails = false;
  disableHot = false;
  mismatchAccounts.clear();
  badCreds.clear();
  shared = false;
  rejectStart = false;
  allExhausted = false;
  messages.length = 0;
  events.length = 0;
  pool = [account("pool-one"), account("pool-two")];
  mocks.spawn.mockReset();
  mocks.kill.mockClear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(
    async (
      command: string,
      args?: { id: string; state: Partial<CodexAccount> },
    ) => {
      if (command === "codex_accounts_list") return pool.map((a) => ({ ...a }));
      if (command === "codex_auth_json_read")
        return {
          accountId: "external",
          accessToken: "fake-external-access",
          refreshToken: "fake-external-refresh",
        };
      if (command === "codex_account_credentials") {
        if (badCreds.has(args!.id)) throw new Error("refresh token dead");
        return {
          ...pool.find((a) => a.id === args!.id),
          accessToken: "fake-pool-access",
          refreshToken: "fake-pool-refresh",
        };
      }
      if (command === "codex_account_update_state")
        Object.assign(
          pool.find((a) => a.id === args!.id)!,
          args!.state,
        );
    },
  );
});
afterEach(async () => {
  await stopCodexSession("failover");
  __codexTestReset();
});

it("hot-switches in place and resends the original turn", async () => {
  await sendCodexTurn(input());
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
  const first = messages
    .filter((m) => m.generation === 1)
    .map((m) => m.method);
  expect(first).toEqual([
    "initialize",
    "initialized",
    "thread/start",
    "turn/start",
    "account/read", // identify the walled external account
    "account/login/start",
    "account/read",
    "turn/start",
  ]);
  expect(
    messages.filter((m) => m.method === "thread/resume"),
  ).toHaveLength(0);
  const turns = messages.filter((m) => m.method === "turn/start");
  expect(turns[1].params).toEqual(turns[0].params);
  expect(events.filter((e) => e.type === "session.error")).toEqual([]);
  expect(events).toContainEqual({
    type: "status",
    text: "Codex account exhausted — switching to pool-one@example.invalid",
  });
  expect(messages.every((m) => m.method !== "account/" + "logout")).toBe(
    true,
  );
});

it("skips a pool account whose credentials cannot be fetched", async () => {
  badCreds.add("pool-one");
  await sendCodexTurn(input());
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
  expect(pool[0].disabledCause).toBeTruthy();
  expect(events).toContainEqual({
    type: "status",
    text: "Codex account exhausted — switching to pool-two@example.invalid",
  });
});

it.each([false, true])(
  "respawns, authenticates before resuming and resends original turn (resume fallback=%s)",
  async (fallback) => {
    disableHot = true;
    resumeFails = fallback;
    await sendCodexTurn(input());
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
    const second = messages
      .filter((m) => m.generation === 2)
      .map((m) => m.method);
    expect(second).toEqual([
      "initialize",
      "initialized",
      "account/login/start",
      "account/read",
      "thread/resume",
      ...(fallback ? ["thread/start"] : []),
      "turn/start",
    ]);
    const turns = messages.filter((m) => m.method === "turn/start");
    expect(turns[1].params).toEqual(turns[0].params);
    expect(events.filter((e) => e.type === "session.error")).toEqual([]);
    expect(events).toContainEqual({
      type: "status",
      text: "Codex account exhausted — switching to pool-one@example.invalid",
    });
    expect(messages.every((m) => m.method !== "account/" + "logout")).toBe(
      true,
    );
    expect(
      mocks.invoke.mock.calls.some(
        ([command]) => command === "codex_account_upsert",
      ),
    ).toBe(false);
    // A subsequent turn stays on the selected process/account.
    await sendCodexTurn(input());
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
  },
);

it("handles a direct turn/start rejection", async () => {
  rejectStart = true;
  await sendCodexTurn(input());
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
});

it("does not rotate for a shared workspace wall", async () => {
  shared = true;
  await expect(sendCodexTurn(input())).rejects.toThrow("shared limit");
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
  expect(
    mocks.invoke.mock.calls.some(
      ([command]) => command === "codex_account_update_state",
    ),
  ).toBe(false);
});

it("keeps empty-pool terminal behavior unchanged", async () => {
  pool = [];
  await sendCodexTurn(input());
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
  expect(events).toContainEqual({
    type: "session.error",
    message: "Quota exhausted",
  });
});

it("bounds switches, persists blocks and reports earliest reset", async () => {
  allExhausted = true;
  await expect(sendCodexTurn(input())).rejects.toThrow("earliest reset");
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
  expect(pool.every((a) => a.blockedUntilMs! > Date.now() + 7000000)).toBe(
    true,
  );
  expect(events.filter((e) => e.type === "session.error")).toHaveLength(1);
});

it("skips a rejected account and recovers on the next candidate", async () => {
  mismatchAccounts.add("pool-one");
  await sendCodexTurn(input());
  expect(mocks.spawn).toHaveBeenCalledTimes(3);
  // The rejected generation never reached thread/resume or turn/start.
  const rejected = messages
    .filter((m) => m.generation === 2)
    .map((m) => m.method);
  expect(rejected).not.toContain("thread/resume");
  expect(rejected).not.toContain("turn/start");
  expect(pool[0].disabledCause).toContain("verification failed");
  expect(
    messages.filter((m) => m.method === "turn/start"),
  ).toHaveLength(2);
});

it("exhausts the pool when every candidate is rejected", async () => {
  mismatchAccounts.add("pool-one");
  mismatchAccounts.add("pool-two");
  await expect(sendCodexTurn(input())).rejects.toThrow("accounts exhausted");
  expect(mocks.spawn).toHaveBeenCalledTimes(3);
  expect(
    messages
      .filter((m) => m.generation === 2)
      .map((m) => m.method),
  ).not.toContain("thread/resume");
});

it("serializes concurrent sends across the failover boundary", async () => {
  await Promise.all([
    sendCodexTurn(input()),
    sendCodexTurn({ ...input(), text: "second request" }),
  ]);
  expect(mocks.spawn).toHaveBeenCalledTimes(1);
  const turns = messages.filter((m) => m.method === "turn/start");
  expect(turns).toHaveLength(3);
  expect(turns[2].params?.input).toEqual([
    { type: "text", text: "second request" },
  ]);
});

it("does not replay the turn when cancelled during respawn", async () => {
  let release!: () => void;
  mocks.spawn.mockResolvedValueOnce(undefined).mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        release = resolve;
      }),
  );
  disableHot = true;
  const turn = sendCodexTurn(input());
  await vi.waitFor(() => expect(generation).toBe(2));
  await cancelCodexTurn("failover");
  release();
  await turn;
  expect(messages.filter((m) => m.method === "turn/start")).toHaveLength(1);
  expect(events.filter((e) => e.type === "session.error")).toEqual([]);
});
