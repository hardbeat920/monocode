import { useMemo } from "react";
import { HarnessIcon } from "../chrome/HarnessIcon";
import { ProjectMascot } from "../chrome/ProjectMascot";
import { TerminalSpinner } from "../chrome/TerminalSpinner";
import { Check, X } from "../chrome/icons";
import { useNow } from "../hooks/useNow";
import type { FilePaneTab } from "../lib/layout";
import { nativeModelId, resolveModel } from "../lib/models";
import {
  findBlockDeep,
  HARNESS_TITLE,
  sessionWorkCwd,
  type Block,
  type HarnessId,
  type Session,
} from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";
import { Shimmer } from "./Shimmer";
import {
  formatElapsed,
  isProseBlock,
  subagentDurationMs,
  subagentReport,
  subagentStatus,
  subagentSteps,
  subagentTypeLabel,
  toolCallLabel,
  type SubagentStatus,
} from "./transcriptActivity";

type Props = {
  file: FilePaneTab;
  sessions: Session[];
  visible: boolean;
  onOpenFile: (path: string) => void;
  onOpenDiff?: (path: string) => void;
};

/**
 * A subagent's transcript in its own pane: the brief it was handed as the
 * prompt, then everything it did, rendered with the same transcript the
 * parent uses, so a subagent's edits and reads look like anyone else's.
 */
export function SubagentSurface({
  file,
  sessions,
  visible,
  onOpenFile,
  onOpenDiff,
}: Props) {
  const source = file.subagent;
  const session = source
    ? sessions.find((entry) => entry.id === source.sessionId)
    : undefined;
  // Looks inside nested transcripts too: a subagent's own subagent opens here.
  // Memoized on the block list: this pane re-renders whenever any session in
  // the window streams, and the lookup only changes when this one's blocks do.
  const blocks = session?.blocks;
  const blockId = source?.blockId;
  const block = useMemo(
    () => (blocks && blockId ? findBlockDeep(blocks, blockId) : undefined),
    [blocks, blockId],
  );

  if (!block || !session) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <p className="font-sans text-[13px] text-content/70">
          This subagent is no longer in the session.
        </p>
      </div>
    );
  }

  return (
    <SubagentView
      block={block}
      session={session}
      visible={visible}
      onOpenFile={onOpenFile}
      onOpenDiff={onOpenDiff}
    />
  );
}

function SubagentView({
  block,
  session,
  visible,
  onOpenFile,
  onOpenDiff,
}: {
  block: Block;
  session: Session;
  visible: boolean;
  onOpenFile: (path: string) => void;
  onOpenDiff?: (path: string) => void;
}) {
  const cwd = sessionWorkCwd(session);
  const meta = block.subagent;
  // The parent turn ending is what settles a subagent the harness never
  // closed out; while the session is busy the call is still live.
  const status = subagentStatus(block, !!session.busy);
  const running = status === "running";
  const title = toolCallLabel(block, cwd);
  const type = subagentTypeLabel(block);
  const model = subagentModel(session.harness, meta?.model, session.model);
  const blocks = useMemo(
    () => subagentTranscript(block, title),
    [block, title],
  );
  const steps = subagentSteps(block).length;

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <header className="flex shrink-0 items-center gap-2.5 border-b border-content/10 px-4 py-2.5">
        <ProjectMascot
          project={block.id}
          active={running}
          className="size-4 shrink-0 text-content/80"
        />
        <div className="min-w-0 flex-1">
          <h2
            className="truncate font-sans text-[13px] font-medium text-content"
            title={title}
          >
            {title}
          </h2>
          <div className="flex min-w-0 items-center gap-1.5 font-sans text-[11px] text-content/50">
            {type ? <span className="shrink-0">{type}</span> : null}
            {type ? <Dot /> : null}
            <HarnessIcon
              harness={session.harness}
              className="size-3 shrink-0"
            />
            <span className="min-w-0 truncate" title={model.title}>
              {model.name}
            </span>
            {meta?.background ? (
              <>
                <Dot />
                <span className="shrink-0">background</span>
              </>
            ) : null}
            {steps > 0 ? (
              <>
                <Dot />
                <span className="shrink-0">
                  {steps} {steps === 1 ? "step" : "steps"}
                </span>
              </>
            ) : null}
          </div>
        </div>
        <StatusBadge block={block} status={status} />
      </header>
      <div className="relative min-h-0 flex-1">
        {blocks.length === 0 ? (
          <Idle running={running} seed={block.id} />
        ) : (
          <AgentTranscript
            blocks={blocks}
            busy={running}
            visible={visible}
            cwd={cwd}
            harness={session.harness}
            model={model.id}
            onOpenFile={onOpenFile}
            onOpenDiff={onOpenDiff}
          />
        )}
      </div>
    </div>
  );
}

function Dot() {
  return (
    <span
      aria-hidden
      className="size-[3px] shrink-0 rounded-full bg-content/25"
    />
  );
}

/** The clock lives here so only the badge re-renders as it ticks. */
function StatusBadge({
  block,
  status,
}: {
  block: Block;
  status: SubagentStatus;
}) {
  const running = status === "running";
  const now = useNow(running);
  const elapsed = formatElapsed(subagentDurationMs(block, now));
  if (status === "running") {
    return (
      <div
        role="status"
        aria-live="polite"
        className="flex shrink-0 items-center gap-1.5 font-sans text-[12px] text-content/50"
      >
        <TerminalSpinner />
        <Shimmer duration={1.2}>
          {elapsed ? `Running for ${elapsed}` : "Running"}
        </Shimmer>
      </div>
    );
  }
  const label =
    status === "completed"
      ? elapsed
        ? `Done in ${elapsed}`
        : "Done"
      : status === "failed"
        ? "Failed"
        : "Stopped";
  const tone = status === "failed" ? "text-red-400" : "text-content/50";
  return (
    <div
      className={`flex shrink-0 items-center gap-1.5 font-sans text-[12px] ${tone}`}
    >
      {status === "completed" ? (
        <Check className="size-3.5" strokeWidth={1.75} />
      ) : (
        <X className="size-3.5" strokeWidth={2} />
      )}
      <span>{label}</span>
    </div>
  );
}

/** Nothing has landed yet: the mascot waits on its own, arcade style. */
function Idle({ running, seed }: { running: boolean; seed: string }) {
  return (
    <div className="grid h-full place-items-center">
      <div className="flex flex-col items-center gap-3 font-sans text-sm text-content/45">
        <ProjectMascot
          project={seed}
          active={running}
          className="size-8 text-content/60"
        />
        {running ? (
          <Shimmer duration={1.6}>Warming up…</Shimmer>
        ) : (
          <span>This subagent left no trail.</span>
        )}
      </div>
    </div>
  );
}

/**
 * The transcript to show: the brief as the user turn, the subagent's own
 * blocks, and — for harnesses that only hand back a result — that result as
 * the answer.
 */
function subagentTranscript(block: Block, title: string): Block[] {
  const meta = block.subagent;
  const nested = meta?.blocks ?? [];
  const prompt = meta?.prompt?.trim() || title;
  const report = subagentReport(block);
  const hasProse = nested.some(isProseBlock);
  // Only a finished run has a duration; a live one would freeze at open time.
  const durationMs =
    meta?.finishedAt != null ? subagentDurationMs(block) : undefined;
  const blocks: Block[] = [];
  if (prompt) {
    blocks.push({
      id: `${block.id}:prompt`,
      role: "user",
      text: prompt,
      ...(meta?.startedAt != null ? { startedAt: meta.startedAt } : {}),
      ...(durationMs != null ? { durationMs } : {}),
    });
  }
  blocks.push(...nested);
  if (report && !hasProse) {
    blocks.push({ id: `${block.id}:report`, role: "assistant", text: report });
  }
  return blocks;
}

/**
 * The subagent's model, if the harness named one the catalog knows, else the
 * raw id it gave us; the session's model when it named none at all.
 */
function subagentModel(
  harness: HarnessId,
  raw: string | undefined,
  fallback: string,
): { id: string; name: string; title: string } {
  if (!raw) {
    const model = resolveModel(harness, fallback);
    return { id: fallback, name: model.name, title: HARNESS_TITLE[harness] };
  }
  const resolved = resolveModel(harness, raw);
  const native = nativeModelId(resolved);
  const matched = !!native && (raw.includes(native) || native.includes(raw));
  return matched
    ? { id: resolved.id, name: resolved.name, title: raw }
    : { id: fallback, name: raw, title: raw };
}
