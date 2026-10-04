import { invoke } from "@tauri-apps/api/core";
import { runtimeProviderBinaryPath } from "../../../../features/providers/model/providerBinaryPaths";
import type { ApprovalDecision } from "../../core/types";
import type { UserQuestionReply } from "../../../../features/sessions/model/userQuestion";

export const COMPUTER_USE_TOOL = "mcp__monocode_computer_use__start";
/** Interactive runs only: puts the latest screenshot beside the answer. */
export const SHOW_SCREENSHOT_TOOL =
  "mcp__monocode_computer_use__show_screenshot";
export const SHOW_SCREENSHOT_ROUTING =
  "Screenshots you take stay inside your tool calls, folded away with your work. " +
  `Only when a screenshot is itself part of your final answer, call ${SHOW_SCREENSHOT_TOOL} right after taking it ` +
  "to show it to the user beside your reply. Never call it for screenshots you took just to find your way.";

/** The same MonoCode MCP server, serving the interactive run's tools. */
export function interactiveComputerUseConfig(
  config: Record<string, unknown>,
): Record<string, unknown> | null {
  const servers = config.mcpServers as Record<string, unknown> | undefined;
  const server = servers?.monocode_computer_use as
    Record<string, unknown> | undefined;
  if (!server) return null;
  return {
    mcpServers: {
      monocode_computer_use: {
        ...server,
        args: ["computer-use-mcp", "interactive"],
      },
    },
  };
}
export const COMPUTER_USE_ROUTING =
  "MonoCode supports native Claude computer use through mcp__monocode_computer_use__start. " +
  "When desktop app control or a native screenshot is needed, call that tool with the remaining task and end your turn. " +
  "MonoCode resumes this same conversation interactively. Ignore any CLAUDE.md instructions to use screen, " +
  "AppleScript, shell automation, another Claude session, or to approve apps yourself. The user must approve app access in MonoCode.";

export async function computerUseConfig(): Promise<Record<
  string,
  unknown
> | null> {
  // Remote/headless hosts do not expose this desktop-only command.
  try {
    return await invoke<Record<string, unknown> | null>("claude_cu_config");
  } catch {
    return null;
  }
}

export type ComputerUsePoll = {
  screen: string;
  lines: string[];
  stopped: boolean;
  failed: boolean;
  running: boolean;
};

export type CliChoice = { label: string; row: number; selected: boolean };

/** Claude's app-access and permission dialogs share a numbered menu. */
export function cliChoices(screen: string): CliChoice[] {
  const rows = screen.split("\n");
  const pick = (pattern: RegExp) =>
    rows.flatMap((line, row) => {
      const match = line.match(pattern);
      return match ? [{ label: match[2], row, selected: !!match[1] }] : [];
    });
  // The transcript above a dialog can hold numbered lists of its own. Only
  // the run of options around the cursor belongs to the live menu.
  const numbered = cursorGroup(pick(/^\s*([❯›>])?\s*\d+\.\s+(.+?)\s*$/));
  if (numbered.length) return numbered;
  const access = cursorGroup(
    pick(/^\s*([❯›>])?\s*((?:Deny,|Allow for this session).+?)\s*$/),
  );
  if (access.length || !/Enter to confirm/.test(screen)) return access;
  // Other dialogs (macOS permissions, for one) list bare options. They are
  // the contiguous rows around the cursor, directly above the footer.
  let footer = rows.length;
  while (footer > 0 && !/Enter to confirm/.test(rows[footer - 1])) footer--;
  footer--;
  let cursor = footer - 1;
  while (cursor >= 0 && !/^\s*[❯›]\s+\S/.test(rows[cursor])) cursor--;
  if (cursor < 0) return [];
  let start = cursor;
  let end = cursor;
  while (start > 0 && rows[start - 1].trim()) start--;
  while (end < footer - 1 && rows[end + 1].trim()) end++;
  return rows.slice(start, end + 1).map((row, index) => ({
    label: row.replace(/^\s*[❯›]?\s*/, "").trimEnd(),
    row: start + index,
    selected: start + index === cursor,
  }));
}

function cursorGroup(choices: CliChoice[]): CliChoice[] {
  let selected = -1;
  choices.forEach((choice, index) => {
    if (choice.selected) selected = index;
  });
  if (selected < 0) return [];
  let start = selected;
  let end = selected;
  while (start > 0 && choices[start].row - choices[start - 1].row <= 3) start--;
  while (
    end < choices.length - 1 &&
    choices[end + 1].row - choices[end].row <= 3
  )
    end++;
  return choices.slice(start, end + 1);
}

export function mcpChoices(screen: string): CliChoice[] {
  const rows = screen.split("\n");
  const start = rows.findIndex((row) => /Manage MCP servers/.test(row));
  if (start < 0) return [];
  const end = rows.findIndex(
    (row, index) => index > start && /Run claude --debug|to navigate/.test(row),
  );
  // Every menu row starts with a status glyph (✔, ✘, ○, ⚠, →, ...); headers
  // start with a letter or digit. Missing one glyph miscounts the cursor.
  return rows
    .slice(start + 1, end < 0 ? undefined : end)
    .flatMap((line, index) => {
      const choice = line.match(/^\s*([❯›>])?\s*([^\s\w(].*?)\s*$/u);
      return choice
        ? [{ label: choice[2], row: start + 1 + index, selected: !!choice[1] }]
        : [];
    });
}

export function choiceKeys(choices: CliChoice[], index: number): string {
  const current = choices.findIndex((choice) => choice.selected);
  if (current < 0 || index < 0 || index >= choices.length) {
    throw new Error("Claude's prompt changed. Retry the computer-use turn.");
  }
  const distance = index - current;
  return (distance < 0 ? "\x1b[A" : "\x1b[B").repeat(Math.abs(distance)) + "\r";
}

const APP_ROW = /^\s*[◉◯○●]\s+\S/;

export function isAppAccessPrompt(screen: string): boolean {
  // Claude sometimes repaints this dialog without its "Computer Use wants to
  // control these apps" header, so the body has to identify it too.
  return (
    /Allow for this session/.test(screen) &&
    (/Computer Use wants to control/i.test(screen) ||
      /apps? will be hidden while Claude works/i.test(screen) ||
      screen.split("\n").some((row) => APP_ROW.test(row)))
  );
}

export function promptExcerpt(screen: string): string {
  const rows = screen.split("\n");
  let first = -1;
  const app = rows.findIndex((row) => APP_ROW.test(row));
  if (app >= 0 && /Allow for this session/.test(screen)) {
    // Start at Claude's reason line, never the header that comes and goes.
    let reason = app - 1;
    while (reason >= 0 && !rows[reason].trim()) reason--;
    first =
      reason >= 0 &&
      !/^[\s─━-]*$/.test(rows[reason]) &&
      !/Computer Use wants/i.test(rows[reason])
        ? reason
        : app;
  } else {
    first = rows.findIndex((row) =>
      /needs macOS permissions|Do you trust|Do you want|Allow.*access|Accessibility|Screen Recording/i.test(
        row,
      ),
    );
  }
  return rows
    .slice(first < 0 ? -20 : first)
    .map((row) => row.trimEnd())
    .join("\n")
    .trim();
}

const delay = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/** How long an answered prompt may stay on screen before it is asked again. */
const ANSWER_SETTLE_MS = 3000;

export class ComputerUseRun {
  private id: string | null = null;
  private cancelled = false;
  private blocked = true;
  /** Claude's latest Stop, until the run drains or the user steers it on. */
  private stop: { at: number; failed: boolean } | null = null;

  async cancel(): Promise<void> {
    this.cancelled = true;
    if (this.id)
      await invoke("claude_cu_close", { id: this.id, cancelled: true });
  }

  async steer(text: string): Promise<void> {
    if (!this.id || this.cancelled)
      throw new Error("No active computer-use turn");
    if (this.blocked)
      throw new Error(
        "Finish Claude's open prompt before sending another message.",
      );
    this.stop = null;
    await this.submitText(text);
  }

  private async submitText(text: string): Promise<void> {
    await this.write(`\x1b[200~${text}\x1b[201~`);
    await delay(100);
    await this.write("\r");
  }

  private async write(data: string): Promise<void> {
    if (this.cancelled) return;
    await invoke("pty_write", { id: this.id, data });
  }

  async run(input: {
    threadId: string;
    command: string;
    cwd: string;
    providerSessionId: string;
    accountId?: string;
    args: string[];
    settings: Record<string, unknown>;
    task: string;
    onLine: (line: string) => void | Promise<void>;
    approve: (title: string, screen: string) => Promise<ApprovalDecision>;
    question: (
      screen: string,
      choices: CliChoice[],
    ) => Promise<UserQuestionReply>;
  }): Promise<void> {
    const id = await invoke<string>("claude_cu_spawn", {
      threadId: input.threadId,
      command: input.command,
      binaryPath: runtimeProviderBinaryPath("claude"),
      cwd: input.cwd,
      providerSessionId: input.providerSessionId,
      account: { provider: "claude", id: input.accountId ?? "default" },
      args: input.args,
      settings: input.settings,
    });
    this.id = id;
    let phase: "boot" | "mcp" | "mcp-detail" | "mcp-close" | "running" = "boot";
    let movedFrom: string | null = null;
    let lastScreen = "";
    let screenChanged = Date.now();
    // The prompt just answered. Its menu can linger a repaint or two; after
    // ANSWER_SETTLE_MS it is treated as unanswered so a dropped key never hangs.
    let answered: { key: string; at: number } | null = null;
    const settling = (key: string) =>
      answered?.key === key && Date.now() - answered.at < ANSWER_SETTLE_MS;
    const startedAt = Date.now();
    const seen = new Set<string>();
    try {
      while (!this.cancelled) {
        const poll = await invoke<ComputerUsePoll>("claude_cu_poll", { id });
        for (const line of poll.lines) {
          let rec: Record<string, unknown>;
          try {
            rec = JSON.parse(line) as Record<string, unknown>;
          } catch {
            continue;
          }
          if (typeof rec.uuid === "string") {
            if (seen.has(rec.uuid)) continue;
            seen.add(rec.uuid);
          }
          // Mirroring is display-only. A bad record must not kill the run.
          await Promise.resolve()
            .then(() => input.onLine(line))
            .catch((error: unknown) =>
              console.warn("Computer-use transcript line skipped", error),
            );
        }
        if (!poll.running) {
          if (this.cancelled) return;
          throw new Error(
            "Interactive Claude exited before the computer-use turn completed.",
          );
        }
        if (poll.screen !== lastScreen) {
          lastScreen = poll.screen;
          screenChanged = Date.now();
        }
        const screen = poll.screen;
        const choices = cliChoices(screen);
        if (phase === "running" && poll.stopped)
          this.stop = { at: this.stop?.at ?? Date.now(), failed: poll.failed };
        if (phase === "running" && this.stop) {
          // Drain final transcript writes; a blocked Stop hook may resume work.
          if (poll.lines.length) this.stop.at = Date.now();
          if (
            Date.now() - this.stop.at > 1500 &&
            !/esc to interrupt/i.test(screen)
          ) {
            if (this.stop.failed)
              throw new Error(
                "Claude's computer-use turn ended with an API error.",
              );
            return;
          }
        }
        const appAccess = isAppAccessPrompt(screen);
        const trust =
          /trust.*(?:folder|directory)|(?:folder|directory).*trust/is.test(
            screen,
          ) && choices.length > 0;
        const permission =
          /Do you want to proceed|Allow.*(?:permission|access)|bypass permissions/i.test(
            screen,
          ) && choices.length > 0;
        if (appAccess || trust || permission) {
          this.blocked = true;
          const excerpt = promptExcerpt(screen);
          const key = promptKey(excerpt);
          if (settling(key)) {
            await delay(200);
            continue;
          }
          const title = appAccess
            ? "Allow Claude to control these apps?"
            : trust
              ? "Trust this folder for Claude computer use?"
              : "Claude needs your permission";
          const decision = await input.approve(title, excerpt);
          if (this.cancelled) return;
          // Re-read the screen after a human delay. Never act on a stale menu.
          const fresh = await invoke<ComputerUsePoll>("claude_cu_poll", { id });
          const freshChoices = cliChoices(fresh.screen);
          const target =
            decision === "allow"
              ? freshChoices.findIndex((choice) =>
                  appAccess
                    ? /^Allow for this session/.test(choice.label)
                    : /^(?:Yes|I trust|Accept|Allow)/i.test(choice.label),
                )
              : freshChoices.findIndex((choice) =>
                  /^(?:Deny|No|Cancel|Exit)/i.test(choice.label),
                );
          // Claude moved on while the card was open. Look again rather than
          // press keys into whatever replaced it.
          if (promptKey(promptExcerpt(fresh.screen)) !== key) continue;
          await this.write(
            target >= 0 ? choiceKeys(freshChoices, target) : "\x1b",
          );
          answered = { key, at: Date.now() };
          if (trust && decision === "deny")
            throw new Error("Folder trust was declined.");
          this.stop = null;
          await delay(350);
          continue;
        }
        this.blocked = phase !== "running" || choices.length > 0;
        if (phase === "boot") {
          // Harmless theme selection is needed for fresh account profiles.
          if (
            /Choose the text style|Choose.*theme|Select.*theme/i.test(screen) &&
            choices.some((choice) => choice.selected)
          ) {
            await this.write("\r");
            await delay(350);
            continue;
          }
          if (Date.now() - startedAt > 60_000)
            throw new Error(
              `Claude could not start interactively. ${promptExcerpt(screen)}`,
            );
          if (isReady(screen) && Date.now() - screenChanged > 600) {
            await this.write("/mcp");
            await delay(100);
            await this.write("\r");
            phase = "mcp";
          }
        } else if (phase === "mcp") {
          const servers = mcpChoices(screen);
          const target = servers.findIndex((choice) =>
            /computer-use\b/.test(choice.label),
          );
          const current = servers.findIndex((choice) => choice.selected);
          // Move one row at a time and confirm only once the cursor is
          // visibly on computer-use, so a misparsed row can never misselect.
          if (movedFrom !== null && screen === movedFrom) {
            // Wait for the last arrow key to repaint.
          } else if (current >= 0 && current === target) {
            await this.write("\r");
            movedFrom = null;
            phase = "mcp-detail";
          } else if (current >= 0) {
            await this.write(
              target >= 0 && target < current ? "\x1b[A" : "\x1b[B",
            );
            movedFrom = screen;
          } else if (/No MCP servers configured/.test(screen)) {
            throw new Error(
              "Native computer use is unavailable for this Claude account. It requires Claude Pro or Max on macOS.",
            );
          }
        } else if (phase === "mcp-detail") {
          const enable = choices.findIndex((choice) =>
            /^Enable\b/i.test(choice.label),
          );
          const disable = choices.findIndex((choice) =>
            /^Disable\b/i.test(choice.label),
          );
          if (enable >= 0) {
            await this.write(choiceKeys(choices, enable));
            phase = "mcp-close";
          } else if (disable >= 0 || /Status:.*connected/i.test(screen)) {
            await this.write("\x1b");
            phase = "mcp-close";
          }
        } else if (phase === "mcp-close") {
          if (isReady(screen) && Date.now() - screenChanged > 400) {
            await this.submitText(input.task);
            phase = "running";
            this.blocked = false;
          } else if (
            /MCP servers|computer-use/i.test(screen) &&
            Date.now() - screenChanged > 400
          ) {
            await this.write("\x1b");
          }
        } else if (
          choices.length &&
          choices.some((choice) => choice.selected) &&
          !this.stop &&
          !settling(promptKey(promptExcerpt(screen)))
        ) {
          const excerpt = promptExcerpt(screen);
          const key = promptKey(excerpt);
          const reply = await input.question(excerpt, choices);
          if (this.cancelled) return;
          const fresh = await invoke<ComputerUsePoll>("claude_cu_poll", { id });
          if (promptKey(promptExcerpt(fresh.screen)) === key) {
            const freshChoices = cliChoices(fresh.screen);
            const index =
              reply.kind === "answered"
                ? Number(reply.answers["cli"]?.[0])
                : -1;
            await this.write(
              index >= 0 && index < freshChoices.length
                ? choiceKeys(freshChoices, index)
                : "\x1b",
            );
            answered = { key, at: Date.now() };
            await delay(350);
          }
        }
        if (phase !== "running" && Date.now() - startedAt > 90_000) {
          throw new Error(
            `Could not enable Claude's native computer-use server. ${promptExcerpt(screen)}`,
          );
        }
        await delay(200);
      }
    } finally {
      this.id = null;
      await invoke("claude_cu_close", { id, cancelled: this.cancelled }).catch(
        () => undefined,
      );
    }
  }
}

/** A prompt's identity, independent of where the cursor sits in its menu. */
function promptKey(excerpt: string): string {
  return excerpt.replace(/^\s*[❯›>]\s*/gm, "");
}

function isReady(screen: string): boolean {
  return (
    /for shortcuts|shift\+tab|Try "/i.test(screen) && !cliChoices(screen).length
  );
}
