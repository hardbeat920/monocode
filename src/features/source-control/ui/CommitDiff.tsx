import { useEffect, useMemo, useRef, useState } from "react";
import { AlertCircle, Loader } from "../../../shared/ui/icons";
import {
  gitCommitFileDiff,
  gitCommitFiles,
  type GitChangedFile,
} from "../../../platform/tauri/fs";
import { forEachConcurrent } from "../../../shared/lib/concurrent";
import type { UnifiedFileDiff } from "../model/unifiedDiff";
import { prepareUnifiedDiff } from "../model/diffWorker";
import { UnifiedDiffView, type UnifiedDiffFileModel } from "./UnifiedDiffView";

type Props = {
  cwd: string;
  sha: string;
  visible?: boolean;
};

type LoadedDiff = {
  binary: boolean;
  tooLarge: boolean;
  unified: UnifiedFileDiff | null;
  error?: string;
};

const DIFF_LOAD_CONCURRENCY = 4;

export function CommitDiff({ cwd, sha, visible = true }: Props) {
  const [files, setFiles] = useState<GitChangedFile[] | null>(null);
  const [diffs, setDiffs] = useState<Map<string, LoadedDiff>>(new Map());
  const [error, setError] = useState<string | null>(null);
  const loadedScope = useRef("");
  // Commit hashes are immutable. Tab visibility must not refetch or reparse
  // an already-complete review; interrupted loads can resume cached files.
  const completedScope = useRef("");
  const diffsRef = useRef(diffs);
  diffsRef.current = diffs;

  useEffect(() => {
    if (!visible) return;
    if (!cwd || cwd === "~" || !sha) {
      setFiles([]);
      setDiffs(new Map());
      return;
    }

    let disposed = false;
    let generation = 0;
    const controller = new AbortController();
    const scope = `${cwd}\0${sha}`;
    const sameScope = loadedScope.current === scope;
    if (!sameScope) {
      completedScope.current = "";
      loadedScope.current = scope;
      setFiles(null);
      setDiffs(new Map());
    }
    if (completedScope.current === scope) return;

    const run = () => {
      const current = ++generation;
      let failed = false;
      void gitCommitFiles(cwd, sha)
        .then(async (listed) => {
          if (disposed || current !== generation) return;
          setFiles(listed);
          const retained = new Set(listed.map((file) => file.relative));
          setDiffs(
            (existing) =>
              new Map([...existing].filter(([id]) => retained.has(id))),
          );
          setError(null);
          await forEachConcurrent(
            listed,
            DIFF_LOAD_CONCURRENCY,
            async (file, index) => {
              const cached = sameScope
                ? diffsRef.current.get(file.relative)
                : undefined;
              if (cached && !cached.error) return;
              let loaded: LoadedDiff;
              try {
                const diff = await gitCommitFileDiff(cwd, sha, file.relative);
                const unified =
                  !diff.binary && !diff.tooLarge
                    ? await prepareUnifiedDiff(
                        diff.original,
                        diff.current,
                        controller.signal,
                        index === 0 ? 0 : 1,
                      )
                    : null;
                loaded = {
                  binary: diff.binary,
                  tooLarge: diff.tooLarge,
                  unified,
                };
              } catch (caught: unknown) {
                failed = true;
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
          if (!disposed && current === generation && !failed)
            completedScope.current = scope;
        })
        .catch((caught: unknown) => {
          if (disposed || current !== generation) return;
          setError(caught instanceof Error ? caught.message : String(caught));
          setFiles([]);
        });
    };

    run();
    return () => {
      disposed = true;
      controller.abort();
    };
  }, [cwd, sha, visible]);

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
              ? `Couldn’t load diff: ${loaded.error}`
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

  const totals = useMemo(() => {
    return models.reduce(
      (sum, file) => ({
        additions: sum.additions + file.additions,
        deletions: sum.deletions + file.deletions,
      }),
      { additions: 0, deletions: 0 },
    );
  }, [models]);

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
        <p className="text-[13px] text-content">Couldn’t load commit</p>
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

  return <UnifiedDiffView files={models} totals={totals} visible={visible} />;
}
