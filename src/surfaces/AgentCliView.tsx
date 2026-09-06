import { useEffect, useRef, useState } from "react";
import { resolveClaudeBinary, resolveCodexBinary } from "../lib/harness/child";
import {
  agentCliLaunch,
  cliCommandInput,
  type AgentCliHarness,
} from "../lib/agentCli";
import { sessionWorkCwd, type Session } from "../lib/session";
import { killPty, type PtyCommand } from "../lib/pty";
import { TerminalView } from "./TerminalView";

type Props = {
  session: Session & { harness: AgentCliHarness };
  initialCommand?: string;
  active: boolean;
  onClose: () => void;
};

/** Run the real installed agent: every native menu, plugin and future slash
 * command remains owned by that agent. Never emulate commands as model text. */
export function AgentCliView({
  session,
  initialCommand = "",
  active,
  onClose,
}: Props) {
  const [launch] = useState(() => ({
    id: `agent-cli-${crypto.randomUUID()}`,
    cwd: sessionWorkCwd(session),
    session,
  }));
  const [command, setCommand] = useState<PtyCommand>();
  const [draft, setDraft] = useState(initialCommand);
  const [error, setError] = useState("");
  const [ready, setReady] = useState(false);
  const [exited, setExited] = useState(false);
  const [sending, setSending] = useState(false);
  const [closing, setClosing] = useState(false);
  const sender = useRef<((text: string) => Promise<void>) | null>(null);
  const name = session.harness === "codex" ? "Codex" : "Claude Code";
  const copied =
    !!launch.session.providerSessionId && !launch.session.pendingSwitch;

  useEffect(() => {
    let cancelled = false;
    const resolve =
      launch.session.harness === "codex"
        ? resolveCodexBinary
        : resolveClaudeBinary;
    void resolve()
      .then(({ path }) => {
        if (!cancelled) setCommand(agentCliLaunch(launch.session, path));
      })
      .catch((err: unknown) => {
        if (!cancelled)
          setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [launch]);

  const send = async () => {
    if (!sender.current || sending || closing) return;
    try {
      const text = cliCommandInput(draft);
      setSending(true);
      setError("");
      await sender.current(text);
      setDraft("");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSending(false);
    }
  };

  return (
    <section
      aria-label={`${name} CLI`}
      className="flex h-full min-h-0 flex-1 flex-col"
    >
      <div className="flex shrink-0 items-center gap-3 border-b border-content/10 px-3 py-2 text-xs">
        <strong>{name} CLI</strong>
        <span className="min-w-0 flex-1 text-content/60">
          {copied
            ? "Separate session with a copy of this chat."
            : "Separate CLI session in this project."}{" "}
          Type / in the terminal for its full command menu.
        </span>
        <button
          type="button"
          disabled={closing}
          className="rounded px-2 py-1 hover:bg-content/10 disabled:opacity-40"
          onClick={() => {
            setClosing(true);
            void killPty(launch.id).then(onClose);
          }}
        >
          Close CLI · Back to chat
        </button>
      </div>
      {draft ? (
        <form
          className="flex shrink-0 items-center gap-2 border-b border-content/10 px-3 py-2"
          onSubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <input
            aria-label="Agent command"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            className="min-w-0 flex-1 rounded bg-content/5 px-2 py-1 font-mono text-sm"
          />
          <span className="text-xs text-content/50">
            Finish any startup prompts below first.
          </span>
          <button
            type="submit"
            disabled={!ready || exited || sending || closing}
            className="rounded bg-accent/15 px-3 py-1 text-xs text-accent disabled:opacity-40"
          >
            Send to CLI
          </button>
        </form>
      ) : null}
      {error ? (
        <p role="alert" className="shrink-0 px-3 py-2 text-sm text-red-400">
          {error}
        </p>
      ) : null}
      <div className="min-h-0 flex-1">
        {command ? (
          <TerminalView
            id={launch.id}
            cwd={launch.cwd}
            command={command}
            active={active}
            onReady={(sendCommand) => {
              sender.current = sendCommand;
              setReady(!!sendCommand);
            }}
            onExit={() => setExited(true)}
          />
        ) : (
          <p className="p-3 text-sm text-content/50">
            {error ? "CLI could not start." : "Starting CLI…"}
          </p>
        )}
      </div>
    </section>
  );
}
