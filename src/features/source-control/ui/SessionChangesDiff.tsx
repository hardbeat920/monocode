import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
} from "react";
import { AlertCircle, Loader } from "../../../shared/ui/icons";
import {
  sessionCheckpointFileDiff,
  sessionCheckpointStatus,
  subscribeReviewChanged,
  type CheckpointFile,
} from "../../sessions/model/checkpoint";
import { forEachConcurrent } from "../../../shared/lib/concurrent";
import type { UnifiedFileDiff } from "../model/unifiedDiff";
import { prepareUnifiedDiff } from "../model/diffWorker";
import { UnifiedDiffView, type UnifiedDiffFileModel } from "./UnifiedDiffView";
import { pruneMap } from "../model/stableDiff";

type Props = {
  cwd: string;
  sessionId: string;
  visible?: boolean;
  focusPath?: string;
  /** Limit the review to these session file paths. */
  paths?: readonly string[];
  /** Which files start open; the focused one always does. */
  initialExpansion?: ComponentProps<typeof UnifiedDiffView>["initialExpansion"];
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

/** Read-only review of the session's recorded before/after snapshots. */
export function SessionChangesDiff({
  cwd,
  sessionId,
  focusPath,
  paths,
  initialExpansion,
  visible = true,
}: Props) {
  const scope = paths == null ? undefined : JSON.stringify(paths);
  // Only the load order uses the focus; moving it must not restart the load.
  const focusRef = useRef(focusPath);
  focusRef.current = focusPath;
  const [files, setFiles] = useState<CheckpointFile[] | null>(null);
  const [diffs, setDiffs] = useState<Map<string, LoadedDiff>>(new Map());
  const [error, setError] = useState<string | null>(null);
  const diffsRef = useRef(diffs);
  diffsRef.current = diffs;
  const loadedScope = useRef("");

  useEffect(() => {
    if (!visible) return;
    if (!cwd || cwd === "~" || !sessionId) {
      setFiles([]);
      setDiffs(new Map());
      return;
    }

    let disposed = false;
    let generation = 0;
    let controller = new AbortController();
    const key = `${cwd}\0${sessionId}\0${scope ?? ""}`;
    const sameScope = loadedScope.current === key;
    if (!sameScope) {
      loadedScope.current = key;
      setFiles(null);
      setDiffs(new Map());
    }
    const run = () => {
      controller.abort();
      controller = new AbortController();
      const { signal } = controller;
      const current = ++generation;
      void sessionCheckpointStatus(sessionId, cwd)
        .then(async (status) => {
          if (disposed || current !== generation) return;
          const allowed =
            scope == null ? null : new Set<string>(JSON.parse(scope));
          const scoped = allowed
            ? status.files.filter((file) => allowed.has(file.path))
            : status.files;
          setFiles(scoped);
          const retained = new Set(scoped.map((file) => file.relative));
          setDiffs((existing) => pruneMap(existing, retained));
          setError(null);
          await forEachConcurrent(
            prioritizeFile(scoped, focusRef.current),
            DIFF_LOAD_CONCURRENCY,
            async (file) => {
              let loaded: LoadedDiff;
              try {
                const diff = await sessionCheckpointFileDiff(
                  sessionId,
                  cwd,
                  file.relative,
                );
                const previous = diffsRef.current.get(file.relative);
                if (
                  previous &&
                  !previous.error &&
                  previous.original === diff.original &&
                  previous.current === diff.current &&
                  previous.binary === diff.binary &&
                  previous.tooLarge === diff.tooLarge
                ) {
                  loaded = previous;
                } else
                  loaded = {
                    binary: diff.binary,
                    tooLarge: diff.tooLarge,
                    original: diff.original,
                    current: diff.current,
                    unified:
                      !diff.binary && !diff.tooLarge
                        ? await prepareUnifiedDiff(
                            diff.original,
                            diff.current,
                            signal,
                            file.path === focusRef.current ? 0 : 1,
                          )
                        : null,
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
              if (disposed || current !== generation) return;
              setDiffs((existing) => {
                if (existing.get(file.relative) === loaded) return existing;
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
      controller.abort();
      unsubscribe();
    };
  }, [cwd, sessionId, scope, visible]);

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
        additions: file.additions,
        deletions: file.deletions,
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
        <p className="text-[13px] text-content">
          Couldn’t load session changes
        </p>
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
    <UnifiedDiffView
      files={models}
      visible={visible}
      focusPath={focusPath}
      totals={totals}
      initialExpansion={initialExpansion}
    />
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
