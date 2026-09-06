import type {
  Attachment,
  RuntimeMode,
  TaskListItem,
  ToolPreview,
  TurnIntent,
} from "../session";
import type { UserQuestion } from "../userQuestion";

export type HarnessEvent =
  | { type: "session.started" }
  | { type: "session.ended"; code?: number | null }
  | { type: "session.error"; message: string }
  | { type: "session.providerBound"; providerSessionId: string }
  | {
      type: "session.configChanged";
      model?: string;
      modelSettings?: Record<string, string>;
    }
  | { type: "status"; text: string }
  | { type: "message.delta"; text: string }
  | { type: "message.completed" }
  | { type: "reasoning.delta"; text: string }
  | { type: "reasoning.completed" }
  | {
      type: "tool.started";
      callId: string;
      title: string;
      kind?: string;
      status?: string;
      preview?: ToolPreview;
      /** Every path affected when one structured edit changes multiple files. */
      paths?: string[];
    }
  | {
      type: "tool.updated";
      callId: string;
      title?: string;
      kind?: string;
      status?: string;
      detail?: string;
      preview?: ToolPreview;
      /** Every path affected when one structured edit changes multiple files. */
      paths?: string[];
    }
  | {
      type: "approval.requested";
      requestId: number;
      title: string;
      kind?: string;
      callId?: string;
      preview?: ToolPreview;
    }
  | {
      type: "approval.resolved";
      requestId: number;
      /** "cancelled" = a PermissionRequest hook decided before the user could. */
      decision: "allow" | "deny" | "cancelled";
    }
  | {
      type: "question.asked";
      requestId: number;
      title?: string;
      questions: UserQuestion[];
      callId?: string;
    }
  | {
      type: "question.resolved";
      requestId: number;
      decision: "answered" | "skipped" | "cancelled";
    }
  | {
      type: "tasks.updated";
      key?: string;
      explanation?: string;
      /** Merge changed items into the existing list instead of replacing it. */
      merge?: boolean;
      items: TaskListItem[];
    }
  | {
      type: "plan";
      text: string;
      /** Merge identity for deltas and the authoritative completed item. */
      key?: string;
      /** Append a stream delta instead of replacing the current snapshot. */
      append?: boolean;
      /** False marks the plan ready for review. */
      streaming?: boolean;
    }
  /** Context-window level after the harness's latest request. */
  | { type: "context"; used?: number; window?: number };

export type ApprovalDecision = "allow" | "deny";

export type HarnessSessionInput = {
  sessionId: string;
  cwd: string;
  model: string;
  modelSettings?: Record<string, string>;
  runtimeMode: RuntimeMode;
  intent?: TurnIntent;
  onEvent: (event: HarnessEvent) => void;
};

export type SendTurnInput = HarnessSessionInput & {
  text: string;
  attachments?: Attachment[];
};

export type CompactContextInput = HarnessSessionInput;

export type SteerTurnInput = {
  sessionId: string;
  cwd: string;
  model: string;
  modelSettings?: Record<string, string>;
  text: string;
  attachments?: Attachment[];
};
