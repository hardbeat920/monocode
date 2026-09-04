import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertCircle, Loader } from "../chrome/icons";
import {
  gitDiffFiles,
  gitDiscardFile,
  gitFileDiff,
  gitStageContents,
  gitStageFile,
  notifyGitChanged,
  subscribeGitChanged,
  type GitChangedFile,
} from "../lib/fs";
import { forEachConcurrent } from "../lib/concurrent";
import { buildUnifiedFile, type UnifiedFileDiff } from "../lib/unifiedDiff";
import { stageChunkText } from "./editorGit";
import { UnifiedDiffView, type UnifiedDiffFileModel } from "./UnifiedDiffView";

type Props = {
  cwd: string;
  focusPath?: string;
};

type LoadedDiff = {
  binary: boolean;
  tooLarge: boolean;
  original: string;
  current: string;
  unified: UnifiedFileDiff | null;
  error?: string;
};

const DIFF_LOAD_CONCURRENCY = 4;
const EMPTY_UNIFIED_DIFF: UnifiedFileDiff = {
  additions: 0,
  deletions: 0,
  lines: [],
  blocks: [],
};

export function WorkingTreeDiff({ cwd, focusPath }: Props) {
  const [files, setFiles] = useState<GitChangedFile[] | null>(null);
  const [diffs, setDiffs] = useState<Map<string, LoadedDiff>>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [totals, setTotals] = useState({ additions: 0, deletions: 0 });
  const diffsRef = useRef(diffs);
  diffsRef.current = diffs;

  useEffect(() => {
    if (!cwd || cwd === "~") {
      setFiles([]);
      setDiffs(new Map());
      return;
    }

    let disposed = false;
    let generation = 0;
    setFiles(null);
    setDiffs(new Map());

    const run = () => {
      const current = ++generation;
      void gitDiffFiles(cwd)
        .then(async (index) => {
          if (disposed || current !== generation) return;
          setTotals({
            additions: index.additions,
            deletions: index.deletions,
          });
          setFiles(index.files);
          setDiffs(new Map());
          setError(null);
          const loadOrder = prioritizeFile(index.files, focusPath);
          await forEachConcurrent(
            loadOrder,
            DIFF_LOAD_CONCURRENCY,
            async (file) => {
              let loaded: LoadedDiff;
              if (!file.unstaged) {
                loaded = {
                  binary: false,
                  tooLarge: false,
                  original: "",
                  current: "",
                  unified: EMPTY_UNIFIED_DIFF,
                };
              } else {
                try {
                  const diff = await gitFileDiff(cwd, file.relative);
                  const unified =
                    !diff.binary && !diff.tooLarge
                      ? buildUnifiedFile(diff.original, diff.current)
                      : null;
                  loaded = {
                    binary: diff.binary,
                    tooLarge: diff.tooLarge,
                    original: diff.original,
                    current: diff.current,
                    unified,
                  };
                } catch (caught: unknown) {
                  loaded = {
                    binary: false,
                    tooLarge: false,
                    original: "",
                    current: "",
                    unified: null,
                    error:
                      caught instanceof Error ? caught.message : String(caught),
                  };
                }
              }
              if (disposed || current !== generation) return;
              setDiffs((existing) => {
                const next = new Map(existing);
                next.set(file.relative, loaded);
                return next;
              });
            },
            () => !disposed && current === generation,
          );
        })
        .catch((caught: unknown) => {
          if (disposed || current !== generation) return;
          setError(caught instanceof Error ? caught.message : String(caught));
          setFiles([]);
        });
    };

    run();
    let refreshFrame = 0;
    const scheduleRun = () => {
      if (refreshFrame) return;
      refreshFrame = window.requestAnimationFrame(() => {
        refreshFrame = 0;
        run();
      });
    };
    const unsub = subscribeGitChanged(scheduleRun);
    const onFocus = () => {
      if (!document.hidden) scheduleRun();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onFocus);
    return () => {
      disposed = true;
      if (refreshFrame) window.cancelAnimationFrame(refreshFrame);
      unsub();
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onFocus);
    };
  }, [cwd]);

  const models = useMemo<UnifiedDiffFileModel[]>(() => {
    if (!files) return [];
    return files.map((file) => {
      const loaded = diffs.get(file.relative);
      const unified = loaded?.unified ?? null;
      const unchanged =
        unified != null &&
        unified.additions === 0 &&
        unified.deletions === 0 &&
        !loaded?.binary;
      return {
        id: file.relative,
        path: file.path,
        label: file.relative,
        binary: loaded?.binary,
        tooLarge: loaded?.tooLarge,
        emptyMessage:
          loaded == null
            ? "Loading…"
            : loaded.error
              ? `Couldn’t load diff: ${loaded.error}`
              : unchanged
                ? file.staged
                  ? "Staged — no unstaged changes"
                  : "No unstaged changes"
                : undefined,
        additions: unified?.additions ?? file.additions,
        deletions: unified?.deletions ?? file.deletions,
        blocks: unchanged ? [] : (unified?.blocks ?? []),
        canStage: file.unstaged,
        canDiscard: file.unstaged,
        canStageHunk: file.unstaged && !loaded?.binary && !loaded?.tooLarge,
      };
    });
  }, [diffs, files]);

  const onStageFile = useCallback(
    async (id: string) => {
      setBusyId(id);
      try {
        await gitStageFile(cwd, id);
        notifyGitChanged();
      } finally {
        setBusyId(null);
      }
    },
    [cwd],
  );

  const onDiscardFile = useCallback(
    async (id: string) => {
      setBusyId(id);
      try {
        await gitDiscardFile(cwd, id);
        notifyGitChanged();
      } finally {
        setBusyId(null);
      }
    },
    [cwd],
  );

  const onStageHunk = useCallback(
    async (id: string, pos: number) => {
      const loaded = diffsRef.current.get(id);
      if (!loaded) return;
      const next = stageChunkText(loaded.original, loaded.current, pos);
      if (next == null) return;
      setBusyId(id);
      try {
        await gitStageContents(cwd, id, next);
        notifyGitChanged();
      } finally {
        setBusyId(null);
      }
    },
    [cwd],
  );

  if (!cwd || cwd === "~") {
    return (
      <p className="grid h-full place-items-center text-[13px] text-content/45">
        No project folder
      </p>
    );
  }
  if (error) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <AlertCircle className="mx-auto mb-3 size-5 text-red-400" />
        <p className="text-[13px] text-content">Couldn’t load changes</p>
        <p className="mt-1 text-[12px] text-content/50">{error}</p>
      </div>
    );
  }
  if (files == null) {
    return (
      <div className="grid h-full place-items-center text-content/40">
        <Loader className="size-4 animate-spin" strokeWidth={1.75} />
      </div>
    );
  }

  return (
    <UnifiedDiffView
      files={models}
      focusPath={focusPath}
      busyId={busyId}
      totals={totals}
      onStageFile={onStageFile}
      onDiscardFile={onDiscardFile}
      onStageHunk={onStageHunk}
    />
  );
}

function prioritizeFile(
  files: readonly GitChangedFile[],
  focusPath: string | undefined,
): GitChangedFile[] {
  if (!focusPath) return [...files];
  const focused = files.find(
    (file) => file.path === focusPath || file.relative === focusPath,
  );
  if (!focused) return [...files];
  return [focused, ...files.filter((file) => file !== focused)];
}
