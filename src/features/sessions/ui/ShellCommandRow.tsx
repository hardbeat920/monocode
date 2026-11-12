import type { Block } from "../model/session";
import { TerminalSpinner } from "./TerminalSpinner";

/** A command the user ran with `!`: theirs, not the agent's, so it gets a card. */
export function ShellCommandRow({
  block,
  embedded,
}: {
  block: Block;
  embedded?: boolean;
}) {
  const shell = block.shell;
  if (!shell) return null;
  const running = shell.status === "running";
  const failed = shell.status === "failed" || (shell.exitCode ?? 0) !== 0;
  const outcome = running
    ? "Running…"
    : shell.status === "stopped"
      ? "Stopped"
      : shell.status === "failed"
        ? "Failed to start"
        : shell.exitCode != null
          ? `Exit ${shell.exitCode}`
          : "Killed";
  const stdout = shell.stdout?.replace(/\n+$/, "");
  const stderr = shell.stderr?.replace(/\n+$/, "");
  return (
    <div className={`${embedded ? "" : "px-4"} py-2`}>
      <div
        data-shell-command={block.id}
        aria-label={`Shell command you ran: ${block.text}`}
        className="overflow-hidden rounded-lg border border-content/10 bg-content/[0.03] font-mono text-[12px] leading-5"
      >
        <div className="flex min-w-0 items-start gap-2 px-3 py-1.5">
          <span aria-hidden="true" className="shrink-0 text-content/40">
            !
          </span>
          <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-content/85">
            {block.text}
          </span>
          <span
            className={`flex shrink-0 items-center gap-1.5 font-sans text-[11px] ${failed ? "text-red-400" : "text-content/45"}`}
          >
            {running ? (
              <TerminalSpinner className="inline-block w-3.5 select-none text-center text-[11px] leading-none" />
            ) : null}
            {outcome}
          </span>
        </div>
        {stdout || stderr ? (
          <pre className="max-h-80 min-w-0 overflow-auto whitespace-pre-wrap break-words border-t border-stroke px-3 py-2 text-content/65">
            {stdout}
            {stdout && stderr ? "\n" : null}
            {stderr ? <span className="text-red-400/80">{stderr}</span> : null}
          </pre>
        ) : null}
        {shell.truncated ? (
          <div className="border-t border-stroke px-3 py-1 font-sans text-[11px] text-content/45">
            Output truncated
          </div>
        ) : null}
      </div>
    </div>
  );
}
