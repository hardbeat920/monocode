import type { PointerEvent as ReactPointerEvent } from "react";
import { memo, useSyncExternalStore } from "react";
import {
  MarkdownViewShell,
  useMarkdownMode,
} from "../../sessions/ui/MarkdownModeToggle";
import { SurfaceTabs } from "../../workspace/ui/SurfaceTabs";
import {
  isAgentTab,
  isChangesTab,
  isCommitTab,
  isPlanTab,
  isReleaseNotesTab,
  isReviewTab,
  isSessionChangesTab,
  isTerminalTab,
  type EditorPane,
  type FilePaneTab,
} from "../../workspace/model/layout";
import { isImagePath } from "../model/filePreview";
import type { TerminalMetaPatch } from "../../terminal/model/terminalTab";
import type { EditorNavigationTarget } from "../../search/model/search";
import { editorPathsEqual } from "../../search/model/search";
import type { PlanBuildTarget, Session } from "../../sessions/model/session";
import { Play } from "../../../shared/ui/icons";
import { BuildTargetButton } from "../../sessions/ui/SecondOpinionButton";
import { loadDiffViewer, subscribeDiffViewer } from "../../settings/model/settings";
import { AgentTabView } from "../../sessions/ui/AgentTabView";
import { MarkdownPreview } from "../../sessions/ui/AgentMarkdown";
import { BinaryFileView } from "./BinaryFileView";
import { CommitDiff } from "../../source-control/ui/CommitDiff";
import { FileEditor } from "./FileEditor";
import { ReleaseNotesSurface } from "../../../app/ui/ReleaseNotesSurface";
import { SessionChangesDiff } from "../../source-control/ui/SessionChangesDiff";
import { TerminalView } from "../../terminal/ui/TerminalView";
import { WorkingTreeDiff } from "../../source-control/ui/WorkingTreeDiff";

type Props = {
  pane: EditorPane;
  focused: boolean;
  /** The title bar already names a standalone file, so avoid repeating it. */
  showTabs?: boolean;
  dirtyFileIds: Set<string>;
  fileErrorCounts: Map<string, number>;
  sessions: Session[];
  onFocus: (paneId: string) => void;
  onSelectFile: (paneId: string, fileId: string) => void;
  onCloseFile: (paneId: string, fileId: string) => void;
  onCloseOtherFiles: (paneId: string, fileId: string) => void;
  onDirtyChange: (fileId: string, dirty: boolean) => void;
  onErrorCountChange: (fileId: string, count: number) => void;
  onReorderFiles: (paneId: string, ids: string[]) => void;
  onOpenFile: (path: string) => void;
  onUpdatePlan: (sessionId: string, blockId: string, text: string) => void;
  onBuildPlan: (
    sessionId: string,
    blockId: string,
    target?: PlanBuildTarget,
  ) => void;
  editorNavigation?: EditorNavigationTarget | null;
  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
  onTerminalMetaChange?: (fileId: string, patch: TerminalMetaPatch) => void;
};

function FilePaneComponent({
  pane,
  focused,
  showTabs = true,
  dirtyFileIds,
  fileErrorCounts,
  sessions,
  onFocus,
  onSelectFile,
  onCloseFile,
  onCloseOtherFiles,
  onDirtyChange,
  onErrorCountChange,
  onReorderFiles,
  onOpenFile,
  onUpdatePlan,
  onBuildPlan,
  editorNavigation,
  onPaneDragStart,
  onTerminalMetaChange,
}: Props) {
  const diffViewer = useSyncExternalStore(
    subscribeDiffViewer,
    loadDiffViewer,
    loadDiffViewer,
  );
  const activeFile = pane.files.find((file) => file.id === pane.activeFileId);
  const sessionReview =
    activeFile && isSessionChangesTab(activeFile) ? activeFile : undefined;
  const unifiedReview =
    !!activeFile &&
    !sessionReview &&
    (isChangesTab(activeFile) ||
      (diffViewer === "unified" && isReviewTab(activeFile)));
  const commitReview = !!activeFile && isCommitTab(activeFile);

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col"
      onMouseDown={() => onFocus(pane.id)}
    >
      {showTabs ? (
        <SurfaceTabs
          files={pane.files}
          activeFileId={pane.activeFileId}
          dirtyFileIds={dirtyFileIds}
          fileErrorCounts={fileErrorCounts}
          onSelectFile={(fileId) => onSelectFile(pane.id, fileId)}
          onCloseFile={(fileId) => onCloseFile(pane.id, fileId)}
          onCloseOtherFiles={(fileId) => onCloseOtherFiles(pane.id, fileId)}
          onReorder={(ids) => onReorderFiles(pane.id, ids)}
          onPaneDragStart={onPaneDragStart}
        />
      ) : null}
      <div className="relative min-h-0 flex-1">
        {sessionReview ? (
          <div className="absolute inset-0 h-full">
            <SessionChangesDiff
              cwd={sessionReview.cwd}
              sessionId={sessionReview.sessionChanges.sessionId}
              focusPath={sessionReview.path}
            />
          </div>
        ) : commitReview && activeFile?.commit ? (
          <div className="absolute inset-0 h-full">
            <CommitDiff cwd={activeFile.cwd} sha={activeFile.commit.sha} />
          </div>
        ) : unifiedReview && activeFile ? (
          <div className="absolute inset-0 h-full">
            <WorkingTreeDiff
              cwd={activeFile.cwd}
              focusPath={activeFile.path}
              focusKind={activeFile.changeKind}
            />
          </div>
        ) : null}
        {pane.files.map((file) => {
          if (
            isCommitTab(file) ||
            isChangesTab(file) ||
            isSessionChangesTab(file) ||
            (unifiedReview && isReviewTab(file))
          )
            return null;
          return (
            <div
              key={file.id}
              aria-hidden={file.id !== pane.activeFileId}
              className={
                file.id === pane.activeFileId
                  ? "absolute inset-0 h-full"
                  : "hidden"
              }
            >
              {isAgentTab(file) ? (
                <AgentTabView
                  title={file.path}
                  session={sessions.find(
                    (entry) => entry.id === file.agent.sessionId,
                  )}
                  visible={file.id === pane.activeFileId}
                  focused={focused && file.id === pane.activeFileId}
                  onOpenFile={onOpenFile}
                />
              ) : isPlanTab(file) ? (
                <PlanSurface
                  file={file}
                  sessions={sessions}
                  onOpenFile={onOpenFile}
                  onUpdatePlan={onUpdatePlan}
                  onBuildPlan={onBuildPlan}
                />
              ) : isReleaseNotesTab(file) ? (
                <ReleaseNotesSurface source={file.releaseNotes} />
              ) : isTerminalTab(file) ? (
                <TerminalView
                  id={file.id}
                  cwd={file.cwd}
                  active={focused && file.id === pane.activeFileId}
                  onMetaChange={(patch) =>
                    onTerminalMetaChange?.(file.id, patch)
                  }
                />
              ) : isImagePath(file.path) ? (
                <BinaryFileView path={file.path} cwd={file.cwd} />
              ) : (
                <FileEditor
                  path={file.path}
                  cwd={file.cwd}
                  showDiff={!!file.review}
                  active={focused && file.id === pane.activeFileId}
                  navigation={
                    editorNavigation &&
                    editorPathsEqual(file.path, editorNavigation.path)
                      ? editorNavigation
                      : null
                  }
                  onDirtyChange={(_path, dirty) =>
                    onDirtyChange(file.id, dirty)
                  }
                  onErrorCountChange={(_path, count) =>
                    onErrorCountChange(file.id, count)
                  }
                  onOpenFile={onOpenFile}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export const FilePane = memo(FilePaneComponent, (previous, next) => {
  if (
    previous.pane !== next.pane ||
    previous.focused !== next.focused ||
    previous.showTabs !== next.showTabs ||
    previous.dirtyFileIds !== next.dirtyFileIds ||
    previous.fileErrorCounts !== next.fileErrorCounts ||
    previous.onFocus !== next.onFocus ||
    previous.onSelectFile !== next.onSelectFile ||
    previous.onCloseFile !== next.onCloseFile ||
    previous.onCloseOtherFiles !== next.onCloseOtherFiles ||
    previous.onDirtyChange !== next.onDirtyChange ||
    previous.onErrorCountChange !== next.onErrorCountChange ||
    previous.onReorderFiles !== next.onReorderFiles ||
    previous.onOpenFile !== next.onOpenFile ||
    previous.onUpdatePlan !== next.onUpdatePlan ||
    previous.onBuildPlan !== next.onBuildPlan ||
    previous.editorNavigation !== next.editorNavigation ||
    Boolean(previous.onPaneDragStart) !== Boolean(next.onPaneDragStart) ||
    previous.onTerminalMetaChange !== next.onTerminalMetaChange
  ) {
    return false;
  }

  for (const file of next.pane.files) {
    // Plans and agent tabs both read a live session object from this pane.
    const sessionId = file.plan?.sessionId ?? file.agent?.sessionId;
    if (!sessionId) continue;
    const before = previous.sessions.find(
      (session) => session.id === sessionId,
    );
    const after = next.sessions.find((session) => session.id === sessionId);
    if (before !== after) return false;
  }
  return true;
});

function PlanSurface({
  file,
  sessions,
  onOpenFile,
  onUpdatePlan,
  onBuildPlan,
}: {
  file: FilePaneTab;
  sessions: Session[];
  onOpenFile: (path: string) => void;
  onUpdatePlan: (sessionId: string, blockId: string, text: string) => void;
  onBuildPlan: (
    sessionId: string,
    blockId: string,
    target?: PlanBuildTarget,
  ) => void;
}) {
  const plan = file.plan;
  const [mode, setMode] = useMarkdownMode(file.path);
  const session = plan
    ? sessions.find((entry) => entry.id === plan.sessionId)
    : undefined;
  const block = plan
    ? session?.blocks.find((entry) => entry.id === plan.blockId)
    : undefined;

  if (!block || !plan) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <p className="text-[13px] text-content/70">
          This plan is no longer in the session.
        </p>
      </div>
    );
  }

  const buildDisabled =
    !!session?.busy ||
    !block.text.trim() ||
    block.plan?.status === "streaming" ||
    block.plan?.status === "building" ||
    block.plan?.status === "built";
  const buildLabel =
    block.plan?.status === "building"
      ? "Building…"
      : block.plan?.status === "built"
        ? "Built"
        : "Build";

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <MarkdownViewShell
        mode={mode}
        onModeChange={setMode}
        preview={
          <MarkdownPreview
            text={block.text}
            streaming={block.streaming}
            cwd={file.cwd}
            onOpenFile={onOpenFile}
          />
        }
        actions={
          <div className="flex items-center font-sans">
            <button
              type="button"
              disabled={buildDisabled}
              onClick={() => onBuildPlan(plan.sessionId, block.id)}
              className={`flex h-6 items-center gap-1.5 bg-content px-2.5 font-sans text-[11px] font-medium text-background-base hover:bg-content/90 disabled:cursor-not-allowed disabled:opacity-40 ${
                session ? "rounded-l-md" : "rounded-md"
              }`}
            >
              <Play className="size-3" />
              {buildLabel}
            </button>
            {session ? (
              <BuildTargetButton
                from={session.harness}
                model={session.model}
                settings={session.modelSettings}
                disabled={buildDisabled}
                onPick={(target) =>
                  onBuildPlan(plan.sessionId, block.id, target)
                }
              />
            ) : null}
          </div>
        }
        source={
          <textarea
            aria-label="Plan markdown"
            spellCheck={false}
            value={block.text}
            disabled={
              block.plan?.status === "streaming" ||
              block.plan?.status === "building" ||
              block.plan?.status === "built"
            }
            onChange={(event) =>
              onUpdatePlan(plan.sessionId, block.id, event.currentTarget.value)
            }
            className="h-full w-full resize-none overflow-auto bg-transparent px-5 pb-5 pt-14 font-mono text-[13px] leading-6 text-content outline-none disabled:opacity-70"
          />
        }
      />
    </div>
  );
}
