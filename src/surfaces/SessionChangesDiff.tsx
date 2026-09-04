import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Loader } from "../chrome/icons";
import {
  sessionCheckpointFileDiff,
  sessionCheckpointStatus,
  subscribeReviewChanged,
  type CheckpointFile,
} from "../lib/checkpoint";
import { forEachConcurrent } from "../lib/concurrent";
import { buildUnifiedFile, type UnifiedFileDiff } from "../lib/unifiedDiff";
import { UnifiedDiffView, type UnifiedDiffFileModel } from "./UnifiedDiffView";

type Props = {
  cwd: string;
  sessionId: string;
  focusPath?: string;
};

type LoadedDiff = {
  binary: boolean;
  tooLarge: boolean;
  unified: UnifiedFileDiff | null;
  error?: string;
};

const DIFF_LOAD_CONCURRENCY = 4;

/** Read-only review of the exact before/after snapshots owned by one session. */
export function SessionChangesDiff({ cwd, sessionId, focusPath }: Props) {
  const [files, setFiles] = useState<CheckpointFile[] | null>(null);
  const [diffs, setDiffs] = useState<Map<string, LoadedDiff>>(new Map());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!cwd || cwd === "~" || !sessionId) {
      setFiles([]);
      setDiffs(new Map());
      return;
    }

    let disposed = false;
    let generation = 0;
    const run = () => {
      const current = ++generation;
      setFiles(null);
      setDiffs(new Map());
      void sessionCheckpointStatus(sessionId, cwd)
        .then(async (status) => {
          if (disposed || current !== generation) return;
          setFiles(status.files);
          setError(null);
          await forEachConcurrent(
            prioritizeFile(status.files, focusPath),
            DIFF_LOAD_CONCURRENCY,
            async (file) => {
              let loaded: LoadedDiff;
              try {
                const diff = await sessionCheckpointFileDiff(
                  sessionId,
                  cwd,
                  file.relative,
                );
                loaded = {
                  binary: diff.binary,
                  tooLarge: diff.tooLarge,
                  unified:
                    !diff.binary && !diff.tooLarge
                      ? buildUnifiedFile(diff.original, diff.current)
                      : null,
                };
              } catch (caught: unknown) {
                loaded = {
                  binary: false,
                  tooLarge: false,
                  unified: null,
                  error:
                    caught instanceof Error ? caught.message : String(caught),
                };
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
    const unsubscribe = subscribeReviewChanged((changedSessionId) => {
      if (!changedSessionId || changedSessionId === sessionId) run();
    });
    return () => {
      disposed = true;
      unsubscribe();
    };
  }, [cwd, focusPath, sessionId]);

  const models = useMemo<UnifiedDiffFileModel[]>(() => {
    if (!files) return [];
    return files.map((file) => {
      const loaded = diffs.get(file.relative);
      const unified = loaded?.unified ?? null;
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
              ? loaded.error
              : unified != null &&
                  unified.additions === 0 &&
                  unified.deletions === 0 &&
                  !loaded.binary
                ? "No textual diff"
                : undefined,
        additions: unified?.additions ?? file.additions,
        deletions: unified?.deletions ?? file.deletions,
        blocks: unified?.blocks ?? [],
      };
    });
  }, [diffs, files]);

  const totals = useMemo(
    () =>
      models.reduce(
        (sum, file) => ({
          additions: sum.additions + file.additions,
          deletions: sum.deletions + file.deletions,
        }),
        { additions: 0, deletions: 0 },
      ),
    [models],
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
        <p className="text-[13px] text-content">Couldn’t load session changes</p>
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
  if (files.length === 0) {
    return (
      <p className="grid h-full place-items-center text-[13px] text-content/45">
        No session changes
      </p>
    );
  }

  return (
    <UnifiedDiffView files={models} focusPath={focusPath} totals={totals} />
  );
}

function prioritizeFile(
  files: readonly CheckpointFile[],
  focusPath: string | undefined,
): CheckpointFile[] {
  if (!focusPath) return [...files];
  const focused = files.find(
    (file) => file.path === focusPath || file.relative === focusPath,
  );
  if (!focused) return [...files];
  return [focused, ...files.filter((file) => file !== focused)];
}
