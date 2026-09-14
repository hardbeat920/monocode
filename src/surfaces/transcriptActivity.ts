import {
  composeToolTitle,
  isAgentTool,
  isEditTool,
  isExecuteTool,
  isReadTool,
  isSearchTool,
  isWeakToolTitle,
} from "../lib/harness/preview";
import { leafName } from "../lib/fileName";
import { displayPath } from "../lib/paths";
import type { Block } from "../lib/session";
import { allModels } from "../lib/models";

export type ToolCallState = "pending" | "accepted" | "rejected";

export type TurnItem =
  | { type: "block"; block: Block }
  | { type: "activity"; blocks: Block[] }
  /** Delegated runs spawned together, kept out of the folding work trail. */
  | { type: "subagents"; blocks: Block[] };

export function needsApproval(block: Block): boolean {
  return !!block.approval && !block.approval.decided;
}

export function toolCallState(block: Block): ToolCallState {
  const status = block.tool?.status?.toLowerCase() ?? "";
  const decided = block.approval?.decided;

  if (decided === "deny") return "rejected";
  if (
    status === "failed" ||
    status === "error" ||
    status === "cancelled" ||
    status === "canceled"
  ) {
    return "rejected";
  }
  if (needsApproval(block)) return "pending";
  if (status === "completed" || status === "success") return "accepted";
  if (
    block.streaming ||
    status === "in_progress" ||
    status === "pending" ||
    status === "running"
  ) {
    return "pending";
  }
  if (decided === "allow" || decided === "cancelled" || !status) {
    return "accepted";
  }
  return "pending";
}

export function toolCallLabel(block: Block, cwd?: string): string {
  const preview = block.tool?.preview;
  const path = preview?.path
    ? displayPath(preview.path, cwd)
    : preview?.fileName;
  return (
    composeToolTitle({
      kind: block.tool?.kind,
      title: block.text || block.tool?.title,
      path,
      query: preview?.query,
      previewKind: preview?.kind,
      cwd,
    }) || "Working"
  );
}

export function isIncompleteTool(
  block: Block,
  label: string,
  state: ToolCallState,
): boolean {
  if (state !== "pending") return false;
  const kind = block.tool?.kind?.toLowerCase();
  if (kind && kind !== "other") return false;
  if (
    block.tool?.preview?.path ||
    block.tool?.preview?.query ||
    block.tool?.preview?.lines?.length
  ) {
    return false;
  }
  return !label || isWeakToolTitle(label);
}

export function isHiddenTool(block: Block): boolean {
  if (block.role !== "tool" && block.role !== "approval") return false;
  // Harnesses also publish todo mutations as ordinary tool calls. The
  // canonical tasks block is the user-facing representation, so keep the
  // provider-internal call out of the activity stack.
  if (block.tool?.kind?.toLowerCase() === "tasks") return true;
  if (
    isEditTool(
      block.tool?.kind,
      block.text || block.tool?.title,
      block.tool?.preview,
    )
  ) {
    return false;
  }
  const state = toolCallState(block);
  return isIncompleteTool(block, toolCallLabel(block), state);
}

/**
 * Foldable work: tool calls, thinking, and edits. An edit still awaiting
 * approval stays out — you cannot judge a diff you cannot see.
 */
export function isActivityBlock(block: Block): boolean {
  if (isThinkingBlock(block)) return true;
  if (block.role !== "tool" && block.role !== "approval") return false;
  if (
    isEditTool(
      block.tool?.kind,
      block.text || block.tool?.title,
      block.tool?.preview,
    ) &&
    needsApproval(block)
  ) {
    return false;
  }
  return !isHiddenTool(block);
}

/** Reasoning the agent streams while it works. */
export function isThinkingBlock(block: Block): boolean {
  return block.role === "reasoning" && !!block.text.trim();
}

export function isToolBlock(block: Block): boolean {
  return block.role === "tool" || block.role === "approval";
}

/** Assistant prose with something in it. */
export function isProseBlock(block: Block): boolean {
  return block.role === "assistant" && !!block.text.trim();
}

/** First paragraph of a folded prose block, stripped to one plain line. */
export function proseSummary(text: string): string {
  const body = text.replace(/```[\s\S]*?(?:```|$)/g, " ");
  const paragraph =
    body
      .split(/\n\s*\n/)
      .map((part) => part.trim())
      .find(Boolean) ?? "";
  return paragraph
    .replace(/^\s{0,3}(?:#{1,6}|>|[-*+]|\d+\.)\s+/gm, "")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__)(.+?)\1/g, "$2")
    .replace(/(\*|_)(.+?)\1/g, "$2")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Canonical verb for a write-preview row, so edits read as "Edit src/app.ts"
 * alongside "Read" and "Find". Harnesses phrase these in past tense, hence the
 * doubled-up forms.
 */
export function editVerb(label: string): string {
  const word = label.trim().split(/\s+/)[0]?.toLowerCase() ?? "";
  if (/^(delete|deleted|remove|removed)$/.test(word)) return "Delete";
  if (/^(move|moved|rename|renamed)$/.test(word)) return "Move";
  if (/^(create|created|add|added|new)$/.test(word)) return "Create";
  if (/^(write|wrote|writing)$/.test(word)) return "Write";
  return "Edit";
}

/** User turns, with handoff dividers sitting on their own row. */
export function groupTurns(blocks: Block[]): Block[][] {
  const turns: Block[][] = [];
  let current: Block[] = [];
  for (const block of blocks) {
    if (block.role === "handoff") {
      if (current.length > 0) turns.push(current);
      turns.push([block]);
      current = [];
      continue;
    }
    if (block.role === "user" && current.length > 0) {
      turns.push(current);
      current = [];
    }
    current.push(block);
  }
  if (current.length > 0) turns.push(current);
  return turns;
}

/**
 * Fold contiguous runs of tool calls and reasoning into activity groups.
 * Assistant prose always stands on its own, including progress updates between
 * groups, so the readable transcript never disappears into activity chrome.
 */
export function groupTurnItems(blocks: Block[]): TurnItem[] {
  const visible = withoutSupersededInitialThinking(
    blocks.filter(
      (block) => !isIgnoredTurnBlock(block) && !isHiddenTool(block),
    ),
  );
  const items: TurnItem[] = [];
  let activity: Block[] = [];
  const flush = () => {
    if (activity.length > 0) {
      items.push({ type: "activity", blocks: activity });
    }
    activity = [];
  };
  visible.forEach((block) => {
    // Delegated runs never join the work trail. Their row is the one thing in
    // a turn that has to stay put: it is where the user goes to watch, and the
    // trail around it folds and re-folds while the subagent is still going.
    if (isSubagentBlock(block)) {
      flush();
      const last = items[items.length - 1];
      if (last?.type === "subagents") last.blocks.push(block);
      else items.push({ type: "subagents", blocks: [block] });
      return;
    }
    if (isActivityBlock(block)) {
      activity.push(block);
      return;
    }
    flush();
    items.push({ type: "block", block });
  });
  flush();
  return items;
}

/**
 * Some harnesses publish private reasoning before their first assistant text.
 * Keep it around only while that text has not arrived; if a tool starts first,
 * the reasoning belongs to that activity group and remains visible there.
 */
function withoutSupersededInitialThinking(blocks: Block[]): Block[] {
  let start = 0;
  while (
    start < blocks.length &&
    (blocks[start].role === "user" || blocks[start].role === "system")
  ) {
    start += 1;
  }

  let end = start;
  while (end < blocks.length && isThinkingBlock(blocks[end])) end += 1;
  if (end === start) return blocks;

  const following = blocks.slice(end);
  const proseIndex = following.findIndex(isProseBlock);
  if (proseIndex < 0) return blocks;
  const toolIndex = following.findIndex(isToolBlock);
  if (toolIndex >= 0 && toolIndex < proseIndex) return blocks;

  return [...blocks.slice(0, start), ...blocks.slice(end)];
}

/** The leading reasoning-only activity shown before the first response arrives. */
export function initialThinkingIndex(items: TurnItem[]): number {
  for (let index = 0; index < items.length; index += 1) {
    const item = items[index];
    if (
      item.type === "block" &&
      (item.block.role === "user" || item.block.role === "system")
    ) {
      continue;
    }
    if (
      item.type === "activity" &&
      item.blocks.length > 0 &&
      item.blocks.every(isThinkingBlock)
    ) {
      return index;
    }
    return -1;
  }
  return -1;
}

function isIgnoredTurnBlock(block: Block): boolean {
  // Keep thinking as a step in the group, so a long think does not read as
  // the agent having stalled.
  if (block.role === "reasoning") return !block.text.trim();
  return block.role === "assistant" && !block.text.trim();
}

/** Text the user actually reads: assistant prose, tasks, and plans, not tool chrome. */
export function turnCopyText(blocks: Block[]): string {
  return blocks
    .filter(
      (block) =>
        block.role === "assistant" ||
        block.role === "tasks" ||
        block.role === "plan",
    )
    .map((block) => block.text.replace(/\r\n?/g, "\n").trim())
    .filter(Boolean)
    .join("\n\n");
}

/**
 * The activity group a settled turn hangs its "Worked for" line on: the last
 * one, which sits right above the final answer.
 */
export function lastActivityIndex(items: TurnItem[]): number {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    if (items[index].type === "activity") return index;
  }
  return -1;
}

/** True while a tool in this turn is still running or waiting on the user. */
export function activityStillRunning(blocks: Block[]): boolean {
  return blocks.some(
    (block) =>
      (isToolBlock(block) &&
        !isHiddenTool(block) &&
        toolCallState(block) === "pending") ||
      needsApproval(block),
  );
}

export function hasRunningSubagent(blocks: Block[]): boolean {
  return blocks.some(
    (block) => isSubagentBlock(block) && toolCallState(block) === "pending",
  );
}

/**
 * A delegated run: the tool call that spawned a subagent. One still waiting on
 * you is not one yet — it stays in the work trail, where its approval controls
 * are.
 */
export function isSubagentBlock(block: Block): boolean {
  return (
    isToolBlock(block) &&
    !needsApproval(block) &&
    isAgentTool(block.tool?.kind, block.text || block.tool?.title)
  );
}

/**
 * The whole brief a run was spawned with. Providers put the instructions here,
 * so this can be a paragraph; it belongs on a tooltip, not on a row.
 */
export function subagentBrief(block: Block): string {
  const name =
    block.agentRun?.name?.trim() ||
    (block.text || block.tool?.title || "").trim();
  const stripped = name
    .replace(/^(?:agent|task|subagent)\b[\s:·-]*/i, "")
    .trim();
  return stripped || "Subagent";
}

/** Past this a name stops being a name and starts being the brief again. */
const MAX_SUBAGENT_NAME = 56;

/**
 * What to call a run on its row. A row is one line next to a live status, so
 * take the brief's first sentence and cap it on a word — the full text stays
 * one hover away.
 */
export function subagentName(block: Block): string {
  const brief = subagentBrief(block);
  const sentence = (brief.match(/^[^.!?]*[.!?]?/)?.[0] ?? brief).trim();
  const name = sentence || brief;
  if (name.length <= MAX_SUBAGENT_NAME) return name;
  const cut = name.slice(0, MAX_SUBAGENT_NAME);
  const space = cut.lastIndexOf(" ");
  const trimmed = space > MAX_SUBAGENT_NAME / 2 ? cut.slice(0, space) : cut;
  return `${trimmed.replace(/[\s,;:]+$/, "")}\u2026`;
}

export function subagentModelName(block: Block): string | undefined {
  const id = block.agentRun?.model?.trim();
  if (!id || /^(?:auto|default|inherit|unspecified)$/i.test(id))
    return undefined;
  return (
    allModels().find((model) => model.id === id || model.nativeId === id)
      ?.name ?? id
  );
}

/**
 * What a delegated run handed back: its report, or the reason it died. The
 * provider puts both in the tool result, so a finished run always has the one
 * thing worth reading at the end of its trail.
 */
export function subagentReport(block: Block): string | undefined {
  if (toolCallState(block) === "pending") return undefined;
  return block.tool?.detail?.trim() || undefined;
}

/** A failed delegated call must stay visible even when the work trail folds. */
export function subagentFailureSummary(blocks: Block[]): string | undefined {
  const failed = blocks.filter(
    (block) =>
      isToolBlock(block) &&
      isAgentTool(block.tool?.kind, block.text || block.tool?.title) &&
      toolCallState(block) === "rejected",
  ).length;
  if (failed === 0) return undefined;
  return failed === 1 ? "Subagent failed" : `${failed} subagents failed`;
}

/**
 * What a run of tool calls was for. Reads and searches are one thing — looking
 * around — so a grep followed by the file it turned up stays one group.
 */
export type ActivityWorkKind = "research" | "edit" | "run" | "agent" | "other";

/** A work kind, or a group the agent only narrated: a thought, or a note. */
export type ActivityPhaseKind = ActivityWorkKind | "think" | "note";

/**
 * One chunk of a turn: the line the agent wrote before it started ("now I need
 * to find the theme provider"), and the calls that line introduced.
 */
export type ActivityPhase = {
  id: string;
  kind: ActivityPhaseKind;
  /** The agent's own words for this run, when it wrote some. */
  headline?: Block;
  steps: Block[];
};

/** Ties break towards the kind that changed the most: an edit outranks a read. */
const WORK_KIND_ORDER: ActivityWorkKind[] = [
  "edit",
  "run",
  "agent",
  "research",
  "other",
];

export function toolCategory(block: Block): ActivityWorkKind {
  const kind = block.tool?.kind;
  const title = block.text || block.tool?.title;
  const preview = block.tool?.preview;
  if (isAgentTool(kind, title)) return "agent";
  if (isEditTool(kind, title, preview)) return "edit";
  if (isSearchTool(kind, title, preview)) return "research";
  if (isReadTool(kind, title, preview)) return "research";
  if (isExecuteTool(kind, title)) return "run";
  return "other";
}

/**
 * Splits a turn's activity into labelled groups. Only the agent saying what it
 * is about to do starts a new one: a run of work is one group however many
 * shapes it takes, so a read, two edits and a test run read as one thing done
 * rather than three rows of chrome.
 */
export function buildActivityPhases(blocks: Block[]): ActivityPhase[] {
  const phases: ActivityPhase[] = [];
  let current: ActivityPhase | undefined;
  const counts = new Map<ActivityWorkKind, number>();

  const open = (kind: ActivityPhaseKind, headline?: Block) => {
    current = { id: headline?.id ?? "", kind, headline, steps: [] };
    counts.clear();
    phases.push(current);
    return current;
  };

  for (const block of blocks) {
    // Reasoning is a step, never a header. The agent's own words title a
    // group; the thinking behind them belongs inside it, where it reads as
    // working out rather than as another thing the agent said.
    if (isThinkingBlock(block)) {
      if (!current) current = open("think");
      current.steps.push(block);
      if (!current.id) current.id = block.id;
      continue;
    }
    if (isProseBlock(block)) {
      const narrating = current?.kind === "think" || current?.kind === "note";
      // A line after work has started is the title of what comes next, not a
      // footnote to what just happened.
      if (!current || !narrating) {
        current = open("note", block);
      } else if (!current.headline) {
        // A group that opened on a thought takes the agent's words as its
        // title, keeping the id it already has so the group is not remounted.
        current.headline = block;
        current.kind = "note";
      } else {
        current.steps.push(block);
      }
      continue;
    }
    if (!current) current = open(toolCategory(block));
    current.steps.push(block);
    // Count each call once. Rescanning the growing group here makes long
    // tool runs quadratic, including work hidden behind a transcript fold.
    if (isToolBlock(block)) {
      const kind = toolCategory(block);
      counts.set(kind, (counts.get(kind) ?? 0) + 1);
      current.kind = dominantCountedWorkKind(counts) ?? current.kind;
    }
    if (!current.id) current.id = block.id;
  }

  return phases;
}

function dominantWorkKind(steps: Block[]): ActivityWorkKind | undefined {
  const counts = new Map<ActivityWorkKind, number>();
  for (const block of steps) {
    if (!isToolBlock(block)) continue;
    const kind = toolCategory(block);
    counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  return dominantCountedWorkKind(counts);
}

function dominantCountedWorkKind(
  counts: ReadonlyMap<ActivityWorkKind, number>,
): ActivityWorkKind | undefined {
  let best: ActivityWorkKind | undefined;
  for (const kind of WORK_KIND_ORDER) {
    const count = counts.get(kind) ?? 0;
    if (count > 0 && (!best || count > (counts.get(best) ?? 0))) best = kind;
  }
  return best;
}

type PhaseTally = {
  /** The kinds of work in the group, in the order the agent first did them. */
  order: ActivityWorkKind[];
  reads: Set<string>;
  edits: Set<string>;
  searches: number;
  runs: number;
  agents: number;
  others: number;
};

function tallySteps(steps: Block[]): PhaseTally {
  const tally: PhaseTally = {
    order: [],
    reads: new Set(),
    edits: new Set(),
    searches: 0,
    runs: 0,
    agents: 0,
    others: 0,
  };
  for (const block of steps) {
    if (!isToolBlock(block)) continue;
    const kind = block.tool?.kind;
    const title = block.text || block.tool?.title;
    const preview = block.tool?.preview;
    const target = preview?.path ?? preview?.fileName ?? block.id;
    const category = toolCategory(block);
    if (!tally.order.includes(category)) tally.order.push(category);
    switch (category) {
      case "edit":
        tally.edits.add(target);
        break;
      case "agent":
        tally.agents += 1;
        break;
      case "run":
        tally.runs += 1;
        break;
      case "research":
        if (isSearchTool(kind, title, preview)) tally.searches += 1;
        else tally.reads.add(target);
        break;
      default:
        tally.others += 1;
    }
  }
  return tally;
}

function fileLabel(paths: Set<string>): string {
  const [first] = paths;
  if (paths.size === 1 && first) return leafName(first) || first;
  return `${paths.size} files`;
}

/** What the calls of one kind add up to: "Edited 2 files", "Ran 3 commands". */
function workSummary(
  kind: ActivityWorkKind,
  tally: PhaseTally,
  live: boolean,
): string {
  switch (kind) {
    case "edit":
      return `${live ? "Editing" : "Edited"} ${fileLabel(tally.edits)}`;
    case "research":
      if (tally.reads.size > 0 && tally.searches === 0) {
        return `${live ? "Reading" : "Read"} ${fileLabel(tally.reads)}`;
      }
      if (tally.reads.size === 0) {
        return live ? "Searching the project" : "Searched the project";
      }
      return live ? "Exploring the project" : "Explored the project";
    case "run":
      return tally.runs === 1
        ? live
          ? "Running a command"
          : "Ran a command"
        : `${live ? "Running" : "Ran"} ${tally.runs} commands`;
    case "agent":
      return tally.agents === 1
        ? live
          ? "Running a subagent"
          : "Ran a subagent"
        : `${live ? "Running" : "Ran"} ${tally.agents} subagents`;
    default:
      return tally.others === 1
        ? live
          ? "Running a tool"
          : "Ran a tool"
        : `${live ? "Running" : "Ran"} ${tally.others} tools`;
  }
}

/** The kind of work the group is in the middle of: its most recent call. */
function currentWorkKind(steps: Block[]): ActivityWorkKind | undefined {
  for (let index = steps.length - 1; index >= 0; index -= 1) {
    if (isToolBlock(steps[index])) return toolCategory(steps[index]);
  }
  return undefined;
}

/**
 * What a run of work adds up to, one clause per kind: "Read 3 files · Edited 2
 * files · Ran a command". While the run is live, the clause for the call in
 * flight is present tense, so "Running 2 commands" settles to "Ran 2 commands"
 * when it folds.
 */
export function workSummaryLine(steps: Block[], live = false): string {
  const tally = tallySteps(steps);
  if (tally.order.length === 0) return live ? "Thinking" : "Thought";
  const running = live ? currentWorkKind(steps) : undefined;
  return tally.order
    .map((kind) => workSummary(kind, tally, kind === running))
    .join(" · ");
}

/** The icon a run of work answers to: whatever it did most of. */
export function workKind(steps: Block[]): ActivityPhaseKind {
  return dominantWorkKind(steps) ?? "think";
}

/**
 * The group's header: the agent's own line if it wrote one, otherwise what the
 * calls add up to.
 */
export function activityPhaseTitle(phase: ActivityPhase, live = false): string {
  const failure = subagentFailureSummary(phase.steps);
  if (failure) return failure;
  if (phase.headline) {
    const summary = proseSummary(phase.headline.text);
    if (summary) return summary;
    return phase.headline.role === "reasoning" ? "Thinking" : "Working";
  }
  return workSummaryLine(phase.steps, live);
}

/** The span of a turn that folds away once the agent has answered for it. */
export type WorkFold = { start: number; end: number };

/**
 * The work a turn can put away: everything from the first thing the agent did
 * up to the last group it has already narrated past, leaving the user's
 * message above and the answer that summarised the work below.
 *
 * Prose following a group puts its work away, except for calls still awaiting
 * approval. As the turn streams, each new paragraph folds the work and running
 * commentary before it, leaving the final answer visible. A late approval can
 * reopen that boundary so its controls remain available.
 */
export function foldableWork(items: TurnItem[]): WorkFold | undefined {
  let end = -1;
  let answered = false;
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type === "activity") {
      if (answered && isFoldableItem(item)) {
        end = index;
        break;
      }
      continue;
    }
    if (item.type === "block" && isProseBlock(item.block)) answered = true;
  }
  if (end < 0) return undefined;
  // Only work and the agent's commentary on it fold. A plan, a task list or a
  // call waiting on approval stays where the agent put it.
  let start = end;
  while (start > 0 && isFoldableItem(items[start - 1])) start -= 1;
  return { start, end };
}

function isFoldableItem(item: TurnItem): boolean {
  // A stack of delegated runs is work, so the fold reaches across it and the
  // turn's status line stays at the top. The rows themselves never collapse —
  // the transcript pins them outside the fold's body.
  if (item.type === "subagents") return true;
  return item.type === "activity"
    ? !item.blocks.some(needsApproval)
    : isProseBlock(item.block);
}

/**
 * Where a turn's work begins, fold or no fold: the line the work folds behind
 * has a place to sit from the start, so it fades in rather than appearing
 * under the reader's eye and shoving the answer down.
 */
export function firstFoldableIndex(items: TurnItem[]): number {
  return items.findIndex(isFoldableItem);
}

/** Every block inside a fold, work and commentary alike. */
export function foldedBlocks(items: TurnItem[], fold: WorkFold): Block[] {
  return items
    .slice(fold.start, fold.end + 1)
    .flatMap((item) =>
      item.type === "block"
        ? [item.block]
        : // Delegated runs keep their own rows, so they are not part of what
          // the fold summarises.
          item.type === "subagents"
          ? []
          : item.blocks,
    );
}

/** True when a nested scroller should consume this wheel, not the parent. */
export function nestedScrollAbsorbsWheel(
  el: { scrollTop: number; scrollHeight: number; clientHeight: number },
  deltaY: number,
): boolean {
  if (el.scrollHeight <= el.clientHeight + 1) return false;
  const atTop = el.scrollTop <= 0;
  const atBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 1;
  return (deltaY < 0 && !atTop) || (deltaY > 0 && !atBottom);
}
