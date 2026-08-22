import { GripVertical, X } from "lucide-react";
import {
  memo,
  useCallback,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { Composer } from "../chrome/Composer";
import { SessionReview } from "../chrome/SessionReview";
import type { ApprovalDecision } from "../lib/harness";
import type { RecentProject } from "../lib/recents";
import {
  sessionDisplayTitle,
  type Attachment,
  type HarnessId,
  type RuntimeMode,
  type Session,
} from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";
import { EmptySession } from "./EmptySession";
import { MOD } from "../lib/platform";

type Props = {
  session: Session;
  visible: boolean;
  focused: boolean;
  inSplit: boolean;
  composerFocused: boolean;
  recents: RecentProject[];
  onFocus: (sessionId: string) => void;
  onClose: (sessionId: string) => void;
  onCwdChange: (sessionId: string, cwd: string) => void;
  onModelChange: (sessionId: string, harness: HarnessId, model: string) => void;
  onModelSettingsChange: (
    sessionId: string,
    settings: Record<string, string>,
  ) => void;
  onRuntimeModeChange: (sessionId: string, mode: RuntimeMode) => void;
  onSubmit: (
    sessionId: string,
    text: string,
    attachments: Attachment[],
  ) => void;
  onStop: (sessionId: string) => void;
  onApproval: (
    sessionId: string,
    requestId: number,
    decision: ApprovalDecision,
  ) => void;
  onOpenFile: (path: string) => void;
  onOpenDiff: (path?: string) => void;
  onOpenPlan: (sessionId: string, blockId: string) => void;
  onNewTerminal: (sessionId: string) => void;
  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
};

export const SessionPane = memo(function SessionPane({
  session,
  visible,
  focused,
  inSplit,
  composerFocused,
  recents,
  onFocus,
  onClose,
  onCwdChange,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
  onSubmit,
  onStop,
  onApproval,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
  onNewTerminal,
  onPaneDragStart,
}: Props) {
  const title = sessionDisplayTitle(session.title, session.harness);
  const approve = useCallback(
    (requestId: number, decision: ApprovalDecision) =>
      onApproval(session.id, requestId, decision),
    [onApproval, session.id],
  );
  const openPlan = useCallback(
    (blockId: string) => onOpenPlan(session.id, blockId),
    [onOpenPlan, session.id],
  );
  const isEmpty = session.blocks.length === 0;
  const dockComposer = !isEmpty || inSplit;
  const composer = (
    <Composer
      enabled={visible}
      focused={focused && composerFocused}
      shell={!dockComposer}
      harness={session.harness}
      model={session.model}
      modelSettings={session.modelSettings}
      runtimeMode={session.runtimeMode}
      cwd={session.cwd}
      recents={recents}
      context={session.context}
      onFocus={() => onFocus(session.id)}
      onCwdChange={(cwd) => onCwdChange(session.id, cwd)}
      onNewTerminal={() => onNewTerminal(session.id)}
      onModelChange={(harness, model) =>
        onModelChange(session.id, harness, model)
      }
      onModelSettingsChange={(settings) =>
        onModelSettingsChange(session.id, settings)
      }
      onRuntimeModeChange={(mode) => onRuntimeModeChange(session.id, mode)}
      onSubmit={(text, attachments) => onSubmit(session.id, text, attachments)}
      onStop={() => onStop(session.id)}
      onOpenFile={onOpenFile}
      busy={!!session.busy}
    >
      <SessionReview
        sessionId={session.id}
        cwd={session.cwd}
        enabled={visible}
        busy={!!session.busy}
        onOpenDiff={onOpenDiff}
      />
    </Composer>
  );

  return (
    <div
      data-session-drop={session.id}
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col"
      onMouseDown={() => onFocus(session.id)}
    >
      {inSplit ? (
        <div
          className={`flex h-9 shrink-0 touch-none items-center gap-1.5 border-b border-content/10 px-2 select-none ${
            onPaneDragStart ? "cursor-grab active:cursor-grabbing" : ""
          }`}
          onPointerDown={(event) => {
            if (event.button !== 0 || !onPaneDragStart) return;
            if (
              (event.target as HTMLElement | null)?.closest("[data-no-drag]")
            ) {
              return;
            }
            onPaneDragStart(event);
          }}
        >
          {onPaneDragStart ? (
            <GripVertical
              className="size-3.5 shrink-0 text-content/35"
              strokeWidth={1.75}
            />
          ) : null}
          <span
            className={`size-2 shrink-0 rounded-full ${focused ? "bg-accent" : "bg-transparent"}`}
          />
          <span
            className="min-w-0 flex-1 truncate text-xs text-content"
            title={title}
          >
            {title}
          </span>
          <button
            type="button"
            title={`Close Pane (${MOD}W)`}
            aria-label="Close pane"
            data-no-drag
            className="grid size-5 shrink-0 place-items-center rounded text-content/50 hover:bg-content/10 hover:text-content"
            onPointerDown={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onClose(session.id);
            }}
          >
            <X className="size-3" strokeWidth={1.75} />
          </button>
        </div>
      ) : null}
      <div className="relative min-h-0 flex-1">
        {isEmpty ? (
          <EmptySession
            cwd={session.cwd}
            composer={dockComposer ? undefined : composer}
          />
        ) : (
          <AgentTranscript
            blocks={session.blocks}
            busy={!!session.busy}
            cwd={session.cwd}
            onApproval={approve}
            onOpenFile={onOpenFile}
            onOpenDiff={onOpenDiff}
            onOpenPlan={openPlan}
          />
        )}
      </div>
      {dockComposer ? (
        <div className="mx-auto w-full max-w-4xl shrink-0">{composer}</div>
      ) : null}
    </div>
  );
});
