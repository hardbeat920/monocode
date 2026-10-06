import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  cliChoices,
  choiceKeys,
  isAppAccessPrompt,
  mcpChoices,
  promptExcerpt,
  ComputerUseRun,
  type ComputerUsePoll,
} from "./claudeComputerUse";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const ready = '❯ Try "edit a file"\n? for shortcuts · shift+tab to cycle';
const servers = `Manage MCP servers
5 servers
claude.ai
❯ ✔ claude.ai Adobe          125 tools
  ✔ claude.ai Claude Docs    8 tools
  ✔ claude.ai Google Drive   11 tools
  → Show unused connectors   1 hidden
Built-in MCPs (always available)
  ✔ computer-use             24 tools
↑/↓ to navigate · Enter to confirm · Esc to cancel`;
const detail =
  "computer-use\nStatus: ✔ connected\n❯ 1. View tools\n  2. Disable\nEsc to back";
const access = `Computer Use wants to control these apps
Open Calculator and take a screenshot.
◉ Calculator
6 other apps will be hidden while Claude works.
❯ Deny, and tell Claude what to do differently (esc)
  Allow for this session (1 app)
Enter to confirm · Esc to cancel`;

describe("native computer-use bridge", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("parses the real CLI's unnumbered app prompt and preserves the default denial", () => {
    const choices = cliChoices(access);
    expect(isAppAccessPrompt(access)).toBe(true);
    expect(choices).toHaveLength(2);
    expect(choices[0].selected).toBe(true);
    expect(choiceKeys(choices, 1)).toBe("\x1b[B\r");
    expect(choiceKeys(choices, 0)).toBe("\r");
  });

  it("finds the app prompt without its header and ignores numbered transcript text", () => {
    // Captured from Claude Code 2.1.289 after it repainted the dialog.
    const screen = `❯ Open Sidemen's channel in Helium
⏺ Plan:
  1. Request access to Helium
  2. Open youtube.com/@sidemen

  Calling computer-use…



   Access Helium to complete the requested task.

     ◉ Helium

   6 other apps will be hidden while Claude works.

   ❯ Deny, and tell Claude what to do differently (esc)
     Allow for this session (1 app)


  Enter to confirm · Esc to cancel`;
    expect(isAppAccessPrompt(screen)).toBe(true);
    const choices = cliChoices(screen);
    expect(choices.map((choice) => choice.label)).toEqual([
      "Deny, and tell Claude what to do differently (esc)",
      "Allow for this session (1 app)",
    ]);
    expect(choiceKeys(choices, 1)).toBe("\x1b[B\r");
    expect(promptExcerpt(screen)).toMatch(/^Access Helium/);
    expect(
      promptExcerpt(`   Computer Use wants to control these apps\n\n${screen}`),
    ).toBe(promptExcerpt(screen));
  });

  it("offers the macOS permission dialog's options", () => {
    // Captured from Claude Code 2.1.289 under MonoCode without TCC grants.
    const screen = `⏺ Calling computer-use · 38s…
────────────────────────────────────────
  Computer Use needs macOS permissions
   Accessibility: ✘ not granted
   Screen Recording: ✘ not granted

   Grant the missing permissions in System Settings, then select "Try again".

   ❯ Open System Settings → Accessibility
     Open System Settings → Screen Recording
     Try again

  Enter to confirm · Esc to cancel`;
    const choices = cliChoices(screen);
    expect(choices.map((choice) => choice.label)).toEqual([
      "Open System Settings → Accessibility",
      "Open System Settings → Screen Recording",
      "Try again",
    ]);
    expect(choiceKeys(choices, 2)).toBe("\x1b[B\x1b[B\r");
    expect(isAppAccessPrompt(screen)).toBe(false);
    expect(promptExcerpt(screen)).toMatch(
      /^Computer Use needs macOS permissions/,
    );
  });

  it("ignores numbered transcript lines when no menu cursor is near them", () => {
    expect(cliChoices("1. First step\n2. Second step\n❯ ")).toEqual([]);
  });

  it("counts the unused-connectors row when selecting the built-in MCP", () => {
    const choices = mcpChoices(servers);
    expect(choices).toHaveLength(5);
    expect(choiceKeys(choices, 4)).toBe("\x1b[B".repeat(4) + "\r");
    expect(cliChoices(detail)[1].label).toBe("Disable");
  });

  it("counts warning rows and stops at the footer", () => {
    const choices = mcpChoices(`Manage MCP servers
❯ ✔ claude.ai Canva            45 tools
  ⚠ claude.ai Excalidraw       needs authentication
  → Show unused connectors     1 hidden
Built-in MCPs (always available)
  ✔ claude-in-chrome           22 tools
  ○ computer-use
  ✘ plugin:github:github
※ Run claude --debug to see error logs
↑/↓ to navigate · Enter to confirm · Esc to cancel`);
    expect(choices.map((choice) => choice.label)).toEqual([
      "✔ claude.ai Canva            45 tools",
      "⚠ claude.ai Excalidraw       needs authentication",
      "→ Show unused connectors     1 hidden",
      "✔ claude-in-chrome           22 tools",
      "○ computer-use",
      "✘ plugin:github:github",
    ]);
  });

  it("refuses to send selection keys when the cursor or requested choice is missing", () => {
    expect(() =>
      choiceKeys([{ label: "Allow", row: 0, selected: false }], 0),
    ).toThrow();
    expect(() => choiceKeys(cliChoices(access), 10)).toThrow();
  });

  function mockCli() {
    let state = "ready";
    let cursor = 0;
    const writes: string[] = [];
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "claude_cu_spawn") return "run-id";
      if (command === "claude_cu_close") return;
      if (command === "pty_write") {
        const data = String(args?.data);
        writes.push(data);
        if (data === "/mcp") state = "mcp-typing";
        else if (data === "\r" && state === "mcp-typing") state = "servers";
        else if (data === "\x1b[B" && state === "servers") cursor++;
        else if (data === "\r" && state === "servers" && cursor === 4)
          state = "detail";
        else if (data === "\x1b" && state === "detail") state = "back";
        else if (data === "\x1b" && state === "back") state = "task-ready";
        else if (data.startsWith("\x1b[200~")) state = "task-typing";
        else if (data === "\r" && state === "task-typing") state = "access";
        else if (state === "access") state = "done";
        return;
      }
      if (command === "claude_cu_poll") {
        const screen =
          state === "servers" || state === "back"
            ? servers
                .replace("❯ ✔ claude.ai Adobe", "  ✔ claude.ai Adobe")
                .split("\n")
                .map((row, index) =>
                  index === 3 + cursor + (cursor >= 4 ? 1 : 0)
                    ? row.replace(/^  /, "❯ ")
                    : row,
                )
                .join("\n")
            : state === "detail"
              ? detail
              : state === "access"
                ? access
                : ready;
        const poll: ComputerUsePoll = {
          screen,
          running: true,
          stopped: state === "done",
          failed: false,
          lines:
            state === "done"
              ? [
                  JSON.stringify({
                    type: "assistant",
                    uuid: "one",
                    message: { content: [{ type: "text", text: "Done" }] },
                  }),
                ]
              : [],
        };
        // Final transcript data is sent only once, as in the native tailer.
        if (state === "done") state = "finished";
        if (state === "finished") poll.stopped = true;
        return poll;
      }
      throw new Error(`Unexpected command ${command}`);
    });
    return writes;
  }

  const input = () => ({
    threadId: "thread",
    command: "/path/claude",
    cwd: "/repo",
    providerSessionId: "provider-id",
    accountId: "account",
    args: ["--permission-mode", "bypassPermissions"],
    settings: {},
    task: "Open Calculator",
    onLine: vi.fn(),
    question: vi.fn(async () => ({ kind: "skipped" as const })),
  });

  it("waits for a human even in full-access mode, then drains and closes the same session", async () => {
    const writes = mockCli();
    let allow!: (value: "allow") => void;
    const approve = vi.fn(
      () =>
        new Promise<"allow">((resolve) => {
          allow = resolve;
        }),
    );
    const options = { ...input(), approve };
    const run = new ComputerUseRun().run(options);
    await vi.advanceTimersByTimeAsync(5000);
    expect(approve).toHaveBeenCalledExactlyOnceWith(
      "Allow Claude to control these apps?",
      access.slice(access.indexOf("\n") + 1),
    );
    expect(writes.at(-1)).toBe("\r"); // Submitted the task; has not approved apps.
    expect(invoke).not.toHaveBeenCalledWith(
      "claude_cu_close",
      expect.anything(),
    );
    allow("allow");
    await vi.advanceTimersByTimeAsync(4000);
    await run;
    expect(writes.at(-1)).toBe("\x1b[B\r");
    expect(options.onLine).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith(
      "claude_cu_spawn",
      expect.objectContaining({
        providerSessionId: "provider-id",
        account: { provider: "claude", id: "account" },
      }),
    );
    expect(invoke).toHaveBeenLastCalledWith("claude_cu_close", {
      id: "run-id",
      cancelled: false,
    });
  });

  it("relays Deny without selecting Allow", async () => {
    const writes = mockCli();
    const run = new ComputerUseRun().run({
      ...input(),
      approve: async () => "deny",
    });
    await vi.advanceTimersByTimeAsync(6000);
    await run;
    expect(writes).not.toContain("\x1b[B\r");
    expect(writes.at(-1)).toBe("\r");
  });

  it("keeps running when mirroring a transcript line fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const writes = mockCli();
    const onLine = vi.fn(async () => {
      throw new Error("Generated image data is not valid base64.");
    });
    const run = new ComputerUseRun().run({
      ...input(),
      onLine,
      approve: async () => "allow",
    });
    await vi.advanceTimersByTimeAsync(6000);
    await expect(run).resolves.toBeUndefined();
    expect(onLine).toHaveBeenCalledTimes(1);
    expect(writes).toContain("\x1b[B\r");
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });

  /** A CLI parked on the app-access prompt, whatever keys arrive. */
  function stuckOnAccess(screenAfterAnswer?: () => string) {
    let answered = false;
    const writes: string[] = [];
    let state = "ready";
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "claude_cu_spawn") return "run-id";
      if (command === "claude_cu_close") return;
      if (command === "pty_write") {
        const data = String(args?.data);
        writes.push(data);
        if (data.startsWith("\x1b[200~")) state = "access";
        return;
      }
      if (command === "claude_cu_poll") {
        const screen =
          state !== "access"
            ? ready
            : answered && screenAfterAnswer
              ? screenAfterAnswer()
              : access;
        return {
          screen,
          running: true,
          stopped: false,
          failed: false,
          lines: [],
        } satisfies ComputerUsePoll;
      }
      throw new Error(`Unexpected command ${command}`);
    });
    return {
      writes,
      markAnswered: () => {
        answered = true;
      },
      enterAccess: () => {
        state = "access";
      },
    };
  }

  it("asks again when an answered prompt never goes away", async () => {
    const cli = stuckOnAccess();
    cli.enterAccess();
    const approve = vi.fn(async () => "allow" as const);
    const runner = new ComputerUseRun();
    // Skip setup: the run is already working when the prompt appears.
    (runner as unknown as { blocked: boolean }).blocked = false;
    const run = runner.run({ ...input(), approve });
    await vi.advanceTimersByTimeAsync(1000);
    expect(approve).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(4000);
    expect(approve).toHaveBeenCalledTimes(2);
    await runner.cancel();
    await vi.advanceTimersByTimeAsync(500);
    await run;
  });

  it("does not press keys into a prompt that changed while the card was open", async () => {
    let allow!: (value: "allow") => void;
    const approve = vi
      .fn<() => Promise<"allow" | "deny">>()
      .mockImplementationOnce(
        () =>
          new Promise<"allow">((resolve) => {
            allow = resolve;
          }),
      )
      .mockResolvedValue("deny");
    const other = access.replace("Calculator", "Finder");
    let swapped = false;
    const cli = stuckOnAccess(() => (swapped ? other : access));
    cli.enterAccess();
    const runner = new ComputerUseRun();
    const run = runner.run({ ...input(), approve });
    await vi.advanceTimersByTimeAsync(500);
    expect(approve).toHaveBeenCalledTimes(1);
    cli.markAnswered();
    swapped = true;
    allow("allow");
    await vi.advanceTimersByTimeAsync(500);
    expect(cli.writes).not.toContain("\x1b[B\r");
    // The new prompt gets its own card.
    expect(approve).toHaveBeenCalledTimes(2);
    await runner.cancel();
    await vi.advanceTimersByTimeAsync(500);
    await run;
  });

  it("cleans up a spawn that finishes after cancellation", async () => {
    let spawned!: (id: string) => void;
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "claude_cu_spawn"
        ? new Promise<string>((resolve) => {
            spawned = resolve;
          })
        : undefined,
    );
    const runner = new ComputerUseRun();
    const run = runner.run({ ...input(), approve: async () => "deny" });
    await runner.cancel();
    spawned("late-id");
    await run;
    expect(invoke).toHaveBeenLastCalledWith("claude_cu_close", {
      id: "late-id",
      cancelled: true,
    });
  });
});
