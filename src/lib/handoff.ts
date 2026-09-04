import { isEditTool } from "./harness/preview";
import { limitSection } from "./jsonText";
import { displayPath } from "./paths";
import {
  HARNESS_TITLE,
  type Block,
  type HarnessId,
  type HandoffMeta,
  type PendingHarnessSwitch,
  type SecondOpinionMeta,
  type Session,
} from "./session";

export const HANDOFF_TITLE = "Handoff";

/** Composer chip: recap is injected on send so the user can add context first. */
export type HandoffComposerCard = {
  from: HarnessId;
  to: HarnessId;
  brief: string;
  request?: string;
  files?: number;
};

export function buildHandoffComposerCard(input: {
  from: HarnessId;
  to: HarnessId;
  brief: string;
  userRequest: string;
  files: string[];
}): HandoffComposerCard {
  const request = input.userRequest.replace(/\s+/g, " ").trim();
  return {
    from: input.from,
    to: input.to,
    brief: input.brief,
    ...(request ? { request: request.slice(0, 240) } : {}),
    ...(input.files.length > 0 ? { files: input.files.length } : {}),
  };
}

export function handoffTurnCard(card: HandoffComposerCard): SecondOpinionMeta {
  return {
    from: card.from,
    to: card.to,
    kind: "handoff",
    ...(card.request ? { request: card.request } : {}),
    ...(card.files != null && card.files > 0 ? { files: card.files } : {}),
  };
}

const USER_LINE_LIMIT = 240;
const ASSISTANT_LIMIT = 500;
const PLAN_LIMIT = 400;
const BRIEF_LIMIT = 1_800;
const REQUEST_LIMIT = 1_500;
const MAX_PRIOR_USERS = 2;
const MIN_AGENT_BRIEF = 40;

export type ComposerSwitchPlan =
  | { kind: "model" }
  | { kind: "empty"; forget: HarnessId }
  | { kind: "revert"; restoreProviderSessionId?: string }
  | { kind: "arm"; pending: PendingHarnessSwitch };

export function planComposerSwitch(
  session: Session,
  next: HarnessId,
): ComposerSwitchPlan {
  if (session.harness === next) return { kind: "model" };
  if (session.pendingSwitch && next === session.pendingSwitch.from) {
    return {
      kind: "revert",
      restoreProviderSessionId: session.pendingSwitch.fromProviderSessionId,
    };
  }
  if (
    !session.blocks.some((block) => block.role === "user") &&
    !session.pendingSwitch
  ) {
    return { kind: "empty", forget: session.harness };
  }
  return {
    kind: "arm",
    pending: session.pendingSwitch ?? {
      from: session.harness,
      fromModel: session.model,
      fromSettings: session.modelSettings,
      ...(session.providerSessionId
        ? { fromProviderSessionId: session.providerSessionId }
        : {}),
    },
  };
}

export function sessionChildHarnesses(session: Session): HarnessId[] {
  const ids = new Set<HarnessId>([session.harness]);
  if (session.pendingSwitch) ids.add(session.pendingSwitch.from);
  const last = lastHandoffBlock(session.blocks)?.handoff;
  if (last?.status === "preparing") ids.add(last.from);
  return [...ids];
}

/** Session as of the end of this turn, so a later turn is not in the recap. */
export function sessionThroughTurn(session: Session, turn: Block[]): Session {
  const lastId = turn[turn.length - 1]?.id;
  if (!lastId) return session;
  const end = session.blocks.findIndex((block) => block.id === lastId);
  if (end < 0) return session;
  return { ...session, blocks: session.blocks.slice(0, end + 1) };
}

export function lastHandoffBlock(blocks: Block[]): Block | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].role === "handoff") return blocks[i];
  }
  return undefined;
}

export function isPreparingHandoff(session: Session): boolean {
  return session.blocks.some(
    (block) =>
      block.role === "handoff" && block.handoff?.status === "preparing",
  );
}

export function pendingHandoff(session: Session): {
  from: HarnessId;
  to: HarnessId;
  text: string;
} | null {
  const last = lastHandoffBlock(session.blocks);
  if (!last?.handoff?.pending || last.handoff.status !== "ready") return null;
  const text = last.text.trim() || buildDeterministicHandoff(session);
  if (!text.trim()) return null;
  return { from: last.handoff.from, to: last.handoff.to, text };
}

export function appendPreparingHandoff(
  session: Session,
  from: HarnessId,
  to: HarnessId,
): Session {
  return appendHandoffBlock(session, {
    from,
    to,
    status: "preparing",
    text: "",
    pending: false,
  });
}

export function appendReadyHandoff(
  session: Session,
  from: HarnessId,
  to: HarnessId,
  text: string,
): Session {
  return appendHandoffBlock(session, {
    from,
    to,
    status: "ready",
    text,
    pending: true,
  });
}

export function completeHandoff(session: Session, text: string): Session {
  const last = lastHandoffBlock(session.blocks);
  if (!last?.handoff) return session;
  const brief = stripGoalSections(text) || buildDeterministicHandoff(session);
  return patchHandoff(
    session,
    last.id,
    { ...last.handoff, status: "ready", pending: true },
    brief,
  );
}

/** User messages already on the transcript after the last handoff divider. */
export function userMessagesAfterHandoff(session: Session): string[] {
  const last = lastHandoffBlock(session.blocks);
  if (!last) return [];
  const start = session.blocks.findIndex((block) => block.id === last.id);
  if (start < 0) return [];
  return session.blocks
    .slice(start + 1)
    .filter((block) => block.role === "user")
    .map((block) => block.text.trim())
    .filter(Boolean);
}

export function consumeHandoff(session: Session): Session {
  const last = lastHandoffBlock(session.blocks);
  if (!last?.handoff?.pending) return session;
  return patchHandoff(session, last.id, { ...last.handoff, pending: false });
}

export function chooseHandoffBrief(
  agentText: string,
  fallback: string,
): string {
  const agent = stripGoalSections(agentText);
  if (agent.length >= MIN_AGENT_BRIEF) {
    return limitSection(agent, BRIEF_LIMIT);
  }
  return limitSection(stripGoalSections(fallback), BRIEF_LIMIT);
}

export function hasSessionEdits(session: Session): boolean {
  return session.blocks.some((block) =>
    isEditTool(
      block.tool?.kind,
      block.text || block.tool?.title,
      block.tool?.preview,
    ),
  );
}

export function shouldAskOutgoingAgent(session: Session): boolean {
  const liveId =
    session.pendingSwitch?.fromProviderSessionId ?? session.providerSessionId;
  return !!liveId && hasSessionEdits(session);
}

export function buildOutgoingHandoffPrompt(userRequest: string): string {
  const request = userRequest.trim() || "(no text)";
  return `The user is switching to another coding agent. Their new message will be sent separately — do not repeat it, and do not add a Goal heading.

<user_request>
${limitSection(request, REQUEST_LIMIT)}
</user_request>

Write a short recap of this conversation so the next agent can continue. Under 120 words. Plain markdown. No title card. No greeting. Do not paste the whole transcript.

Rules:
- Use only this conversation.
- Do not run git, do not inspect the working tree, do not read files, do not call tools.
- Mention files only if this chat edited them.
- If the chat was a greeting or has no task yet, say that in one sentence. Do not invent work from uncommitted repo files.

Include only sections that have session-specific content:
- Session so far (a few bullets, not a transcript)
- Files edited in this session
- Suggested next step`;
}

export function buildDeterministicHandoff(
  session: Session,
  request?: string,
  cwd = session.cwd,
): string {
  const current = request?.trim() ?? "";
  const users: string[] = [];
  let lastAssistant = "";
  let lastTasks = "";
  let lastPlan = "";
  const files = new Map<string, string>();

  for (const block of session.blocks) {
    if (block.role === "handoff" || block.role === "reasoning") continue;
    if (block.role === "user") {
      const text = block.text.trim();
      if (text) users.push(text);
      continue;
    }
    if (block.role === "assistant") {
      const text = block.text.trim();
      if (text) lastAssistant = text;
      continue;
    }
    if (block.role === "plan") {
      const text = block.text.trim();
      if (text) lastPlan = text;
      continue;
    }
    if (block.role === "tasks") {
      const text = block.text.trim();
      if (text) lastTasks = text;
      continue;
    }
    if (block.role === "tool" || block.role === "approval") {
      if (
        !isEditTool(
          block.tool?.kind,
          block.text || block.tool?.title,
          block.tool?.preview,
        )
      ) {
        continue;
      }
      const label = toolHandoffLine(block, cwd);
      if (label) files.set(label.toLowerCase(), label);
    }
  }

  const priorAll =
    current && users[users.length - 1] === current
      ? users.slice(0, -1)
      : users;
  const omitted = Math.max(0, priorAll.length - MAX_PRIOR_USERS);
  const prior = priorAll.slice(-MAX_PRIOR_USERS);

  const sections: string[] = [];
  if (omitted > 0 || prior.length > 0 || lastAssistant) {
    const lines = [
      omitted > 0 ? `(${omitted} earlier messages omitted)` : "",
      ...prior.map((text) =>
        `User: ${oneLine(limitSection(text, USER_LINE_LIMIT))}`,
      ),
      lastAssistant
        ? `Assistant: ${oneLine(limitSection(lastAssistant, ASSISTANT_LIMIT))}`
        : "",
    ].filter(Boolean);
    if (lines.length > 0) {
      sections.push(`## Session so far\n${lines.join("\n")}`);
    }
  }
  if (files.size > 0) {
    sections.push(
      `## Files edited in this session\n${[...files.values()]
        .slice(0, 40)
        .map((line) => `- ${line}`)
        .join("\n")}`,
    );
  }
  if (lastPlan) {
    sections.push(`## Plan\n${limitSection(lastPlan, PLAN_LIMIT)}`);
  }
  if (lastTasks) {
    sections.push(`## Current tasks\n${limitSection(lastTasks, PLAN_LIMIT)}`);
  }

  return limitSection(sections.join("\n\n").trim(), BRIEF_LIMIT);
}

export function wrapHandoffPrompt(
  brief: string,
  from: HarnessId,
  userText: string,
  earlierRequests: string[] = [],
): string {
  const fromTitle = HARNESS_TITLE[from];
  const request = userText.trim();
  const body = stripGoalSections(brief);
  const earlier = earlierRequests.map((text) => text.trim()).filter(Boolean);
  const earlierBlock =
    earlier.length > 0
      ? `\n\nAfter the switch, before this message, the user also sent:\n\n${earlier.join("\n\n")}`
      : "";
  const lead = `You are continuing an existing conversation handed off from ${fromTitle}. This is not a new session. Do not say you have no prior context.\n\n${request}${earlierBlock}`;
  if (!body) {
    return `${lead}\n\nContinue from a ${fromTitle} session. Do not invent prior work.`;
  }
  return `${lead}

Prior conversation from ${fromTitle} — this is the thread you are joining, not optional background:

<handoff>
${body}
</handoff>`;
}

function appendHandoffBlock(
  session: Session,
  input: HandoffMeta & { text: string },
): Session {
  return {
    ...session,
    blocks: [
      ...session.blocks,
      {
        id: crypto.randomUUID(),
        role: "handoff",
        text: input.text,
        handoff: {
          from: input.from,
          to: input.to,
          status: input.status,
          ...(input.pending ? { pending: true } : {}),
        },
      },
    ],
  };
}

function patchHandoff(
  session: Session,
  id: string,
  handoff: HandoffMeta,
  text?: string,
): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) => {
      if (block.id !== id) return block;
      return {
        ...block,
        ...(text != null ? { text } : {}),
        handoff,
      };
    }),
  };
}

function toolHandoffLine(block: Block, cwd?: string): string | undefined {
  const preview = block.tool?.preview;
  const path = preview?.path
    ? displayPath(preview.path, cwd)
    : preview?.fileName;
  const title = (block.text || block.tool?.title || "").trim();
  if (path && title) {
    const lower = title.toLowerCase();
    if (lower.includes(path.toLowerCase())) return title;
    return `${title} (${path})`;
  }
  return path || title || undefined;
}

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** Drop a Goal heading so the new message is not duplicated in the recap. */
export function stripGoalSections(markdown: string): string {
  const trimmed = markdown.trim();
  if (!trimmed) return "";
  const withoutHeading = trimmed
    .split(/(?=^#{1,6}\s)/m)
    .filter((chunk) => !/^#{1,6}\s*Goal\b/i.test(chunk.trimStart()))
    .join("")
    .trim();
  return withoutHeading.replace(/^Goal:\s.*(?:\n|$)/im, "").trim();
}
