import type { ApprovalDecision } from "../../../integrations/harness";
import type {
  Attachment,
  PlanBuildTarget,
  Session,
} from "../../sessions/model/session";
import type { UserQuestionReply } from "../../sessions/model/userQuestion";
import { displayAttachments } from "../../sessions/model/attachments";
import { listMonos, monoLook } from "./mono";
import { pixelLayers } from "../../projects/model/pixelMascots";

export const FLOATING_MONO_CHANGED = "mono_chat_changed";
export const FLOATING_MONO_REQUEST = "mono_chat_request";
export type FloatingMonoEntry = {
  id: string;
  name: string;
  mascot: string;
  color: string;
  sessionId: string | null;
  planMode?: boolean;
};
export type FloatingMonoView = {
  monos: FloatingMonoEntry[];
  monoId: string | null;
  session: Session | null;
  error: string | null;
};
export type FloatingMonoAction =
  | { kind: "open" }
  | { kind: "submit"; text: string; attachments: Attachment[] }
  | { kind: "planMode"; enabled: boolean }
  | { kind: "openPlan"; blockId: string }
  | { kind: "buildPlan"; blockId: string; target?: PlanBuildTarget }
  | { kind: "stop" }
  | { kind: "create" }
  | { kind: "approval"; requestId: number; decision: ApprovalDecision }
  | { kind: "question"; requestId: number; reply: UserQuestionReply }
  | { kind: "questionInteraction"; requestId: number }
  | { kind: "reveal" }
  | { kind: "openFile"; path: string }
  | { kind: "openArtifact"; id: string }
  | { kind: "resume" };
export type FloatingMonoRequest = {
  id: number;
  monoId: string;
  action: FloatingMonoAction;
};

export function floatingMonoRoster(enabled: boolean): FloatingMonoEntry[] {
  return enabled
    ? listMonos().map((mono) => ({
        id: mono.id,
        ...monoLook(mono),
        sessionId: mono.sessionId ?? null,
        planMode: mono.planMode === true,
      }))
    : [];
}

/** Draw the same portraits in AppKit without a second set of mascot assets. */
export function floatingMonoMenuMascots(monos: FloatingMonoEntry[]) {
  return monos.map((mono) => {
    const { body, open } = pixelLayers(mono.mascot, mono.color);
    return { monoId: mono.id, rects: [...body, ...open] };
  });
}

/** Blob URLs belong to their webview; send attachment bytes/paths instead. */
export function floatingMonoSession(session: Session): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) =>
      block.attachments?.length
        ? { ...block, attachments: floatingMonoAttachments(block.attachments) }
        : block,
    ),
    queuedMessages: session.queuedMessages?.map((message) => ({
      ...message,
      attachments: floatingMonoAttachments(message.attachments),
    })),
  };
}

export function floatingMonoAttachments(
  attachments: Attachment[],
): Attachment[] {
  return displayAttachments(attachments).map(
    ({ previewUrl: _preview, ...file }) => file,
  );
}

export type FloatingMonoHost = {
  open(monoId: string): Promise<Session | undefined>;
  setPlanMode(
    monoId: string,
    enabled: boolean,
    sessionId: string,
  ): boolean | void;
  openPlan(sessionId: string, blockId: string): boolean | void;
  buildPlan(sessionId: string, blockId: string, target?: PlanBuildTarget): void;
  submit(
    sessionId: string,
    text: string,
    attachments: Attachment[],
  ): boolean | void;
  stop(sessionId: string): void;
  approval(
    sessionId: string,
    requestId: number,
    decision: ApprovalDecision,
  ): void;
  question(
    sessionId: string,
    requestId: number,
    reply: UserQuestionReply,
  ): void;
  questionInteraction(sessionId: string, requestId: number): void;
  reveal(monoId: string): Promise<void>;
  openFile(path: string): void | Promise<void>;
  openArtifact?(monoId: string, id: string): void | Promise<void>;
  resume(sessionId: string): void;
  /** Add a Mono and show it in place of the chat that asked. */
  create?(fromMonoId: string): Promise<void>;
};

export function canChangeMonoPlanMode(session: Session | undefined): boolean {
  return (
    !!session &&
    !session.busy &&
    !session.backgroundTasks?.length &&
    !session.queuedMessages?.length
  );
}

/** Preparation may await disk; recheck the receipt before mutating a session. */
export async function deliverFloatingMonoRequest(
  request: FloatingMonoRequest,
  host: FloatingMonoHost,
  accept: () => Promise<boolean>,
): Promise<Session | undefined> {
  const session = await host.open(request.monoId);
  if (!session) throw new Error("That Mono is no longer available.");
  if (!(await accept())) return undefined;
  const action = request.action;
  switch (action.kind) {
    case "open":
      break;
    case "submit":
      if (host.submit(session.id, action.text, action.attachments) === false)
        throw new Error(
          "The message could not be sent. Try again in a moment.",
        );
      break;
    case "planMode":
      if (
        host.setPlanMode(request.monoId, action.enabled, session.id) === false
      )
        throw new Error(
          "Plan mode can only be changed while the Mono is idle.",
        );
      break;
    case "openPlan":
      if (host.openPlan(session.id, action.blockId) === false)
        throw new Error("That plan is no longer available.");
      break;
    case "buildPlan":
      host.buildPlan(session.id, action.blockId, action.target);
      break;
    case "stop":
      host.stop(session.id);
      break;
    case "approval":
      host.approval(session.id, action.requestId, action.decision);
      break;
    case "question":
      host.question(session.id, action.requestId, action.reply);
      break;
    case "questionInteraction":
      host.questionInteraction(session.id, action.requestId);
      break;
    case "reveal":
      await host.reveal(request.monoId);
      break;
    case "openFile":
      await host.openFile(action.path);
      break;
    case "openArtifact":
      if (!host.openArtifact) throw new Error("Artifacts are unavailable.");
      await host.openArtifact(request.monoId, action.id);
      break;
    case "resume":
      host.resume(session.id);
      break;
    case "create":
      if (!host.create) throw new Error("New Monos are unavailable here.");
      await host.create(request.monoId);
      break;
    default:
      throw new Error("Unknown chat action.");
  }
  return session;
}
