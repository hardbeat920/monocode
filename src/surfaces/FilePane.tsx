import type { PointerEvent as ReactPointerEvent } from "react";
import { memo, useSyncExternalStore } from "react";
import {
  MarkdownViewShell,
  useMarkdownMode,
} from "../chrome/MarkdownModeToggle";
import { SurfaceTabs } from "../chrome/SurfaceTabs";
import {
  isChangesTab,
  isCommitTab,
  isPlanTab,
  isReleaseNotesTab,
  isReviewTab,
  isTerminalTab,
  type EditorPane,
  type FilePaneTab,
} from "../lib/layout";
import { isImagePath } from "../lib/filePreview";
import type { TerminalMetaPatch } from "../lib/terminalTab";
import type { EditorNavigationTarget } from "../lib/search";
import { editorPathsEqual } from "../lib/search";
import type { Session } from "../lib/session";
import { loadDiffViewer, subscribeDiffViewer } from "../lib/settings";
import { MarkdownPreview, MarkdownSource } from "./AgentMarkdown";
import { BinaryFileView } from "./BinaryFileView";
import { CommitDiff } from "./CommitDiff";
import { FileEditor } from "./FileEditor";
import { ReleaseNotesSurface } from "./ReleaseNotesSurface";
import { TerminalView } from "./TerminalView";
import { WorkingTreeDiff } from "./WorkingTreeDiff";

type Props = {
  pane: EditorPane;
  focused: boolean;
  dirtyFileIds: Set<string>;
  fileErrorCounts: Map<string, number>;
  sessions: Session[];
  onFocus: (paneId: string) => void;
  onSelectFile: (paneId: string, fileId: string) => void;
  onCloseFile: (paneId: string, fileId: string) => void;
  onDirtyChange: (fileId: string, dirty: boolean) => void;
  onErrorCountChange: (fileId: string, count: number) => void;
  onReorderFiles: (paneId: string, ids: string[]) => void;
  onOpenFile: (path: string) => void;
  editorNavigation?: EditorNavigationTarget | null;
  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
  onTerminalMetaChange?: (fileId: string, patch: TerminalMetaPatch) => void;
};

function FilePaneComponent({
  pane,
  focused,
  dirtyFileIds,
  fileErrorCounts,
  sessions,
  onFocus,
  onSelectFile,
  onCloseFile,
  onDirtyChange,
  onErrorCountChange,
  onReorderFiles,
  onOpenFile,
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
  const unifiedReview =
    !!activeFile &&
    (isChangesTab(activeFile) ||
      (diffViewer === "unified" && isReviewTab(activeFile)));
  const commitReview = !!activeFile && isCommitTab(activeFile);

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col"
      onMouseDown={() => onFocus(pane.id)}
    >
      <SurfaceTabs
        files={pane.files}
        activeFileId={pane.activeFileId}
        dirtyFileIds={dirtyFileIds}
        fileErrorCounts={fileErrorCounts}
        onSelectFile={(fileId) => onSelectFile(pane.id, fileId)}
        onCloseFile={(fileId) => onCloseFile(pane.id, fileId)}
        onReorder={(ids) => onReorderFiles(pane.id, ids)}
        onPaneDragStart={onPaneDragStart}
      />
      <div className="relative min-h-0 flex-1">
        {commitReview && activeFile?.commit ? (
          <div className="absolute inset-0 h-full">
            <CommitDiff cwd={activeFile.cwd} sha={activeFile.commit.sha} />
          </div>
        ) : unifiedReview && activeFile ? (
          <div className="absolute inset-0 h-full">
            <WorkingTreeDiff cwd={activeFile.cwd} focusPath={activeFile.path} />
          </div>
        ) : null}
        {pane.files.map((file) => {
          if (
            isCommitTab(file) ||
            isChangesTab(file) ||
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
              {isPlanTab(file) ? (
                <PlanSurface
                  file={file}
                  sessions={sessions}
                  onOpenFile={onOpenFile}
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
                  onDirtyChange={(_path, dirty) => onDirtyChange(file.id, dirty)}
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
    previous.dirtyFileIds !== next.dirtyFileIds ||
    previous.fileErrorCounts !== next.fileErrorCounts ||
    previous.onFocus !== next.onFocus ||
    previous.onSelectFile !== next.onSelectFile ||
    previous.onCloseFile !== next.onCloseFile ||
    previous.onDirtyChange !== next.onDirtyChange ||
    previous.onErrorCountChange !== next.onErrorCountChange ||
    previous.onReorderFiles !== next.onReorderFiles ||
    previous.onOpenFile !== next.onOpenFile ||
    previous.editorNavigation !== next.editorNavigation ||
    Boolean(previous.onPaneDragStart) !== Boolean(next.onPaneDragStart) ||
    previous.onTerminalMetaChange !== next.onTerminalMetaChange
  ) {
    return false;
  }

  for (const file of next.pane.files) {
    const sessionId = file.plan?.sessionId;
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
}: {
  file: FilePaneTab;
  sessions: Session[];
  onOpenFile: (path: string) => void;
}) {
  const plan = file.plan;
  const [mode, setMode] = useMarkdownMode(file.path);
  const session = plan
    ? sessions.find((entry) => entry.id === plan.sessionId)
    : undefined;
  const block = plan
    ? session?.blocks.find((entry) => entry.id === plan.blockId)
    : undefined;

  if (!block) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <p className="text-[13px] text-content/70">
          This plan is no longer in the session.
        </p>
      </div>
    );
  }

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
        source={<MarkdownSource text={block.text} />}
      />
    </div>
  );
}
