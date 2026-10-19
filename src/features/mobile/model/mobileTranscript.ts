import { monoReactionBlocks } from "../../monos/model/monoReaction";
import { visibleUserPrompt } from "../../orchestration/model/orchestration";
import { isEmojiOnlyMessage } from "../../sessions/model/emojiMessage";
import { resolveModel } from "../../sessions/model/models";
import { monoWorkStatus } from "../../sessions/model/monoWorkStatus";
import {
  isOperatorUserTurn,
  operatorUserPrompt,
} from "../../sessions/model/operatorCommand";
import {
  hasPendingApproval,
  type Block,
  type HarnessId,
  type Session,
  type TurnMetrics,
} from "../../sessions/model/session";
import {
  activityPhaseTitle,
  buildActivityPhases,
  firstFoldableIndex,
  foldableWork,
  isNoticeBlock,
  isProseBlock,
  monoTurnLatestStart,
  monoTurnRuns,
  needsApproval,
  opensNewStretch,
  toolCallLabel,
  toolCallState,
  isToolBlock,
  proseSummary,
  turnCopyText,
  workSummaryLine,
  type ActivityPhaseKind,
} from "../../sessions/model/transcriptActivity";
import { TranscriptTurnCache } from "../../sessions/model/transcriptTurnCache";

// The phone renders what AgentTranscript would, from turns the desktop has
// already grouped and folded, so both follow one set of rules.

export type MobileStep = { id: string; label: string; failed?: boolean };
export type MobilePhase = {
  id: string;
  kind: ActivityPhaseKind;
  title: string;
  steps: MobileStep[];
};
/** What an opened fold holds, in order: the agent's notes between its work. */
export type MobileFoldEntry =
  | { type: "prose"; id: string; text: string }
  | ({ type: "phase" } & MobilePhase);
export type MobileTurnItem =
  | {
      type: "user";
      id: string;
      text: string;
      at?: number;
      reaction?: string;
      emojiOnly?: boolean;
      attachments?: number;
      /** Another message follows straight on, so this one drops its time. */
      compact?: boolean;
    }
  | { type: "text"; id: string; text: string; streaming?: boolean }
  /** The turn's status line: the fold line, or a Mono's signature. */
  | {
      type: "header";
      id: string;
      live: boolean;
      /** Shown as is. Without one the phone formats the clock itself. */
      label?: string;
      waiting?: string;
      startedAt?: number;
      durationMs?: number;
      model?: string;
      harness?: HarnessId;
      /** The work the line folds away; empty when there is none to open. */
      entries: MobileFoldEntry[];
    }
  /** Work left open in the turn: live, or after the last answer. */
  | { type: "work"; id: string; live: boolean; phases: MobilePhase[] }
  | { type: "notice"; id: string; text: string; error?: boolean };

export type MobileTurn = {
  id: string;
  /** A day stamp above the turn, for a Mono's conversation kept over days. */
  stampAt?: number;
  items: MobileTurnItem[];
  /** When the answer landed, for the line under a settled turn. */
  completedAt?: number;
  footer?: boolean;
  /** What the footer's copy button copies: the turn's replies, as on desktop. */
  copyText?: string;
  /** Token accounting for the footer's metrics button; Monos leave it out. */
  metrics?: TurnMetrics;
  durationMs?: number;
};

const MAX_TURNS = 30;
const MAX_STEPS = 40;

type Options = {
  busy: boolean;
  mono?: { name: string };
  pendingQuestion?: boolean;
  backgroundTasks?: string[];
  /**
   * Which turns to build, as [start, end) over every turn's ID; the latest
   * MAX_TURNS without one. Grouping covers the whole session, but building a
   * turn's items is the costly part, so only the ones sent are built.
   */
  range?: (turnIds: string[]) => [number, number];
};

export function mobileTranscript(
  session: Session,
  options: Options,
): MobileTurn[] {
  const inlineWork = !!options.mono;
  const blocks = inlineWork
    ? monoReactionBlocks(session.blocks)
    : session.blocks;
  const cache = new TranscriptTurnCache();
  const turns = cache.group(blocks, false, inlineWork);
  const activeTurnIndex = turns.reduce(
    (latest, turn, index) =>
      turn[0].monoHabit || turn[0].role === "handoff" ? latest : index,
    -1,
  );
  const model = resolveModel(session.harness, session.model).name;
  const waiting = hasPendingApproval(blocks)
    ? "Waiting for approval"
    : options.pendingQuestion
      ? "Waiting for answers"
      : undefined;
  const [first, end] = options.range?.(turns.map((turn) => turn[0].id)) ?? [
    Math.max(0, turns.length - MAX_TURNS),
    turns.length,
  ];
  return turns.slice(first, end).map((turn, offset) => {
    const index = first + offset;
    const runs = inlineWork ? monoTurnRuns(turn) : [turn];
    const userBlock = inlineWork
      ? monoTurnUserBlock(runs[0])
      : turnUserBlock(turn);
    const lastRun =
      runs.length > 1 ? monoTurnUserBlock(runs[runs.length - 1]) : userBlock;
    const habit = turn[0].monoHabit;
    const agentName = options.mono?.name;
    const standaloneReply =
      !userBlock && !!(agentName || habit) && turn.some(isProseBlock);
    const durationMs =
      runs.length > 1
        ? sumDurations(runs.map((run) => monoTurnUserBlock(run)?.durationMs))
        : userBlock?.durationMs;
    const settled = !(
      options.busy &&
      !standaloneReply &&
      index === activeTurnIndex
    );
    const items = cache.turnItems(turn, settled, { inlineWork });
    const startedAt = userBlock?.startedAt ?? habit?.at ?? turn[0].startedAt;
    const previous = turns[index - 1];
    const previousAt = !previous
      ? undefined
      : inlineWork
        ? monoTurnLatestStart(previous)
        : (turnUserBlock(previous)?.startedAt ??
          previous[0].monoHabit?.at ??
          previous[0].startedAt);
    const stampAt =
      inlineWork && startedAt != null && opensNewStretch(startedAt, previousAt)
        ? startedAt
        : undefined;
    const live = !settled;
    const reactedOnly =
      inlineWork &&
      !live &&
      items.every(
        (item) => item.type === "block" && item.block.role === "user",
      ) &&
      items.some((item) => item.type === "block" && !!item.block.monoReaction);
    const fold = inlineWork ? undefined : foldableWork(items);
    const firstWork =
      habit || standaloneReply
        ? 0
        : inlineWork
          ? items.findIndex(
              (item) => item.type !== "block" || item.block.role !== "user",
            )
          : firstFoldableIndex(items);
    const showHeader =
      !reactedOnly &&
      (!!habit ||
        standaloneReply ||
        live ||
        durationMs != null ||
        (inlineWork ? firstWork >= 0 : !!fold));
    const headerAt = fold
      ? fold.start
      : firstWork >= 0
        ? firstWork
        : items.length;

    const header = (): MobileTurnItem => {
      const base = {
        type: "header" as const,
        id: `${turn[0].id}:header`,
        live,
        startedAt,
        durationMs,
        harness: userBlock?.turnModel?.harness ?? session.harness,
        model:
          agentName ?? userBlock?.turnModel?.name ?? (live ? model : undefined),
        entries: fold
          ? items
              .slice(fold.start, fold.end + 1)
              .flatMap((item): MobileFoldEntry[] =>
                item.type === "block"
                  ? isProseBlock(item.block)
                    ? [
                        {
                          type: "prose",
                          id: item.block.id,
                          text: item.block.text,
                        },
                      ]
                    : []
                  : phasesOf(item.blocks, session.cwd).map((phase) => ({
                      type: "phase",
                      ...phase,
                    })),
              )
          : [],
      };
      if (habit || standaloneReply)
        return { ...base, label: agentName ?? habit?.name };
      if (inlineWork) {
        if (!live) return { ...base, label: agentName };
        const activity = turn.filter(
          (block) => !block.internal && !block.draft,
        );
        let label = monoWorkStatus(activity, true).label;
        if (options.pendingQuestion && !activity.some(needsApproval))
          label = "Waiting for answers…";
        else if (options.backgroundTasks?.length)
          label =
            options.backgroundTasks.length === 1
              ? "Waiting for background task…"
              : `Waiting for ${options.backgroundTasks.length} background tasks…`;
        return { ...base, label };
      }
      if (live) return { ...base, waiting };
      if (durationMs != null) return base;
      return {
        ...base,
        label: workSummaryLine(
          fold
            ? items
                .slice(fold.start, fold.end + 1)
                .flatMap((item) =>
                  item.type === "activity" ? item.blocks : [],
                )
            : [],
        ),
      };
    };

    const out: MobileTurnItem[] = [];
    items.forEach((item, itemIndex) => {
      if (showHeader && itemIndex === headerAt) out.push(header());
      if (fold && itemIndex >= fold.start && itemIndex <= fold.end) return;
      if (item.type !== "block") {
        // A Mono's process lives in its status line; only what needs you shows.
        if (inlineWork) {
          for (const block of item.blocks.filter(needsApproval))
            out.push({
              type: "notice",
              id: block.id,
              text: `Waiting for approval on desktop: ${toolCallLabel(block, session.cwd)}`,
            });
          return;
        }
        const active = live && itemIndex === items.length - 1;
        out.push({
          type: "work",
          id: item.blocks[0].id,
          live: active,
          phases: phasesOf(item.blocks, session.cwd, active),
        });
        return;
      }
      const converted = blockItem(item.block, session.cwd);
      if (converted) out.push(converted);
    });
    if (showHeader && headerAt >= items.length) out.push(header());
    // Messages stacked one after another share the last one's time.
    out.forEach((entry, at) => {
      if (entry.type === "user" && out[at + 1]?.type === "user")
        entry.compact = true;
    });

    const footer =
      settled &&
      !reactedOnly &&
      (durationMs != null || standaloneReply || (inlineWork && firstWork >= 0));
    const completedAt =
      habit?.at ??
      (runs.length > 1 && lastRun?.startedAt != null
        ? lastRun.startedAt + (lastRun.durationMs ?? 0)
        : startedAt != null
          ? startedAt + (durationMs ?? 0)
          : undefined);
    return {
      id: turn[0].id,
      ...(stampAt != null ? { stampAt } : {}),
      items: out,
      ...(footer
        ? {
            footer: true,
            completedAt,
            copyText: turnCopyText(
              inlineWork
                ? items.flatMap((item) =>
                    item.type === "block" ? [item.block] : [],
                  )
                : turn,
            ),
            ...(durationMs != null ? { durationMs } : {}),
            ...(!inlineWork && userBlock?.turnMetrics
              ? { metrics: userBlock.turnMetrics }
              : {}),
          }
        : {}),
    };
  });
}

function blockItem(block: Block, cwd: string): MobileTurnItem | null {
  if (block.role === "user") {
    const text = visibleUserPrompt(
      isOperatorUserTurn(block) ? operatorUserPrompt(block) : block.text,
    );
    if (!text && !block.attachments?.length) return null;
    return {
      type: "user",
      id: block.id,
      text,
      at: block.sentAt ?? block.startedAt,
      ...(block.monoReaction ? { reaction: block.monoReaction } : {}),
      ...(text && isEmojiOnlyMessage(text) ? { emojiOnly: true } : {}),
      ...(block.attachments?.length
        ? { attachments: block.attachments.length }
        : {}),
    };
  }
  if (isProseBlock(block))
    return {
      type: "text",
      id: block.id,
      text: block.text,
      ...(block.streaming ? { streaming: true } : {}),
    };
  if (block.role === "plan" && !block.orchestration && block.text.trim())
    return { type: "text", id: block.id, text: block.text };
  if (block.role === "tasks" && block.taskList?.items.length)
    return {
      type: "text",
      id: block.id,
      text: block.taskList.items
        .map(
          (task) =>
            `- ${task.status === "completed" ? "~~" + task.text + "~~" : task.text}`,
        )
        .join("\n"),
    };
  if (isToolBlock(block))
    return {
      type: "notice",
      id: block.id,
      text: needsApproval(block)
        ? `Waiting for approval on desktop: ${toolCallLabel(block, cwd)}`
        : toolCallLabel(block, cwd),
    };
  if (block.role === "handoff")
    return { type: "notice", id: block.id, text: block.text || "Handed off" };
  if (block.role === "system" && block.text.trim())
    return {
      type: "notice",
      id: block.id,
      text: block.text,
      ...(isNoticeBlock(block) && block.notice === "error"
        ? { error: true }
        : {}),
    };
  return null;
}

function phasesOf(blocks: Block[], cwd: string, live = false): MobilePhase[] {
  const phases = buildActivityPhases(blocks);
  let budget = MAX_STEPS;
  return phases.map((phase, index) => {
    const active = live && index === phases.length - 1;
    const steps = phase.steps.slice(-Math.max(0, budget));
    budget -= steps.length;
    const rows = steps.map((step) => ({
      id: step.id,
      label: isToolBlock(step)
        ? toolCallLabel(step, cwd)
        : proseSummary(step.text) || "Thinking",
      ...(isToolBlock(step) && toolCallState(step) === "rejected"
        ? { failed: true }
        : {}),
    }));
    // As ActivityPhaseGroup: a lone call the agent never introduced is not a
    // group, just its own row.
    if (!phase.headline && rows.length === 1)
      return {
        id: phase.id,
        kind: phase.kind,
        title: rows[0].label,
        steps: [],
      };
    return {
      id: phase.id,
      kind: phase.kind,
      title: activityPhaseTitle(phase, active),
      steps: rows,
    };
  });
}

/** As AgentTranscript: the turn's own prompt, past any app-written ones. */
function turnUserBlock(blocks: Block[]): Block | undefined {
  return [...blocks]
    .reverse()
    .find((block) => block.role === "user" && !block.internal);
}

/** As AgentTranscript: the run's latest started prompt, hidden ones included. */
function monoTurnUserBlock(blocks: Block[]): Block | undefined {
  return (
    [...blocks]
      .reverse()
      .find((block) => block.role === "user" && block.startedAt != null) ??
    blocks.find((block) => block.role === "user")
  );
}

function sumDurations(durations: (number | undefined)[]): number | undefined {
  const known = durations.filter((value): value is number => value != null);
  return known.length
    ? known.reduce((sum, value) => sum + value, 0)
    : undefined;
}
