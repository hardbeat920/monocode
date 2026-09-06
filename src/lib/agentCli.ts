import codexCommands from "./codexCliCommands.json";
import claudeCommands from "./claudeCliCommands.json";
import type { HarnessId, Session } from "./session";
import type { Skill } from "./skills";
import type { PtyCommand } from "./pty";

export type AgentCliHarness = "codex" | "claude";
export type SlashFilter = "all" | "commands" | "skills";
const FILTER_KEY = "monocode.slashFilter";

export function supportsAgentCli(
  harness: HarnessId,
): harness is AgentCliHarness {
  return harness === "codex" || harness === "claude";
}

// Documented command names, 2026-09-06. The installed CLI owns availability,
// execution, custom commands and future commands; these are completion hints.
// https://learn.chatgpt.com/docs/developer-commands
// https://code.claude.com/docs/en/commands
export function agentCliCommands(harness: HarnessId): Skill[] {
  if (!supportsAgentCli(harness)) return [];
  const names = harness === "codex" ? codexCommands : claudeCommands;
  return names.map((name) => ({
    kind: "native",
    name,
    invocation: `cli:${name}`,
    aliases: [name],
    description: `Open /${name} in ${harness === "codex" ? "Codex" : "Claude Code"} CLI`,
    source: harness,
    origin: "CLI",
  }));
}

export function loadSlashFilter(): SlashFilter {
  try {
    const value = localStorage.getItem(FILTER_KEY);
    return value === "commands" || value === "skills" ? value : "all";
  } catch {
    return "all";
  }
}

export function saveSlashFilter(filter: SlashFilter): void {
  try {
    localStorage.setItem(FILTER_KEY, filter);
  } catch {
    /* private mode */
  }
}

export function filterSlashItems(items: Skill[], filter: SlashFilter): Skill[] {
  return items.filter(
    (item) =>
      filter === "all" ||
      (filter === "commands"
        ? item.kind === "native"
        : item.kind === "file" || item.name === "create-skill"),
  );
}

/** Explicit /cli:name works in every filter. Skills and MonoCode shortcuts
 * retain their existing meaning unless the user selects Agent commands. */
export function agentCliPrompt(
  text: string,
  harness: HarnessId,
  filter: SlashFilter,
  skills: Skill[],
): string | null {
  if (!supportsAgentCli(harness)) return null;
  const match = text.match(/^\s*\/([a-zA-Z0-9_.:-]+)(?=\s|$)/);
  if (!match) return null;
  const name = match[1];
  if (name.startsWith("cli:") && name.length > 4) {
    return text.trimStart().replace(/^\/cli:/, "/");
  }
  if (filter === "skills") return null;
  if (filter === "commands") return text.trimStart();
  if (
    name === "plan" ||
    name === "compact" ||
    skills.some((skill) => skill.invocation === name)
  )
    return null;
  const names = harness === "codex" ? codexCommands : claudeCommands;
  return names.includes(name) ? text.trimStart() : null;
}

/** Native sessions are forks, never a second writer to the chat's history.
 * Pass argv directly to the PTY host; no shell quoting or prompt argument. */
export function agentCliLaunch(
  session: Pick<Session, "harness" | "providerSessionId" | "pendingSwitch">,
  binary: string,
): PtyCommand {
  if (!supportsAgentCli(session.harness))
    throw new Error("This agent has no native CLI view.");
  if (!binary.trim()) throw new Error("Agent executable was not found.");
  const resume = !session.pendingSwitch && session.providerSessionId;
  return {
    program: binary,
    args: resume
      ? session.harness === "codex"
        ? ["fork", resume]
        : ["--resume", resume, "--fork-session"]
      : [],
  };
}

/** Terminal escape sequences must never become keyboard input from a draft. */
export function cliCommandInput(text: string): string {
  if (
    !/^\/[a-zA-Z0-9_.:-]+(?:\s[^\r\n]*)?$/.test(text.trim()) ||
    /[\x00-\x1f\x7f]/.test(text)
  ) {
    throw new Error(
      "Enter one slash command without line breaks or control characters.",
    );
  }
  return text.trim();
}
