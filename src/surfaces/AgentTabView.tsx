import { AgentTranscript } from "./AgentTranscript";
import { HarnessIcon } from "../chrome/HarnessIcon";
import { findModel } from "../lib/models";
import { HARNESS_TITLE, sessionWorkCwd, type Session } from "../lib/session";

/**
 * One orchestration worker, watched from its lead's workspace.
 *
 * Read-only on purpose: the run belongs to the orchestrator, which decides
 * what each worker is asked and when. A composer here would put a second
 * voice into a conversation the lead is holding, so the way to change course
 * is to say so in the lead's own transcript.
 */
export function AgentTabView({
  title,
  session,
  visible,
  onOpenFile,
}: {
  title: string;
  session?: Session;
  visible: boolean;
  onOpenFile?: (path: string) => void;
}) {
  if (!session) {
    return (
      <div className="grid h-full place-items-center px-6 text-center">
        <p className="max-w-sm text-[12px] leading-5 text-content/45">
          This agent is no longer running. Its work is summarised in the
          orchestrator's conversation.
        </p>
      </div>
    );
  }
  const model = findModel(session.model)?.name ?? session.model;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        <AgentTranscript
          blocks={session.blocks}
          busy={session.busy}
          cwd={sessionWorkCwd(session)}
          harness={session.harness}
          model={session.model}
          visible={visible}
          onOpenFile={onOpenFile}
          managed
        />
      </div>
      <footer className="flex shrink-0 items-center gap-1.5 border-t border-content/10 px-3 py-1.5 font-sans text-[11px] text-content/45">
        <HarnessIcon harness={session.harness} className="size-3.5 shrink-0" />
        <span className="min-w-0 truncate" title={title}>
          {model} · {HARNESS_TITLE[session.harness]}
        </span>
        <span className="ml-auto shrink-0">
          Run by the orchestrator · read-only
        </span>
      </footer>
    </div>
  );
}
