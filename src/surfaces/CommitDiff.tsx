import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Loader } from "../chrome/icons";
import {
  gitCommitFileDiff,
  gitCommitFiles,
  type GitChangedFile,
} from "../lib/fs";
import { buildUnifiedFile } from "../lib/unifiedDiff";
import {
  UnifiedDiffView,
  type UnifiedDiffFileModel,
} from "./UnifiedDiffView";

type Props = {
  cwd: string;
  sha: string;
};

type LoadedDiff = {
  file: GitChangedFile;
  binary: boolean;
  tooLarge: boolean;
  original: string;
  current: string;
};

export function CommitDiff({ cwd, sha }: Props) {
  const [files, setFiles] = useState<GitChangedFile[] | null>(null);
  const [diffs, setDiffs] = useState<Map<string, LoadedDiff>>(new Map());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!cwd || cwd === "~" || !sha) {
      setFiles([]);
      setDiffs(new Map());
      return;
    }

    let disposed = false;
    let generation = 0;

    const run = () => {
      const current = ++generation;
      void gitCommitFiles(cwd, sha)
        .then(async (listed) => {
          if (disposed || current !== generation) return;
          setFiles(listed);
          setError(null);
          const entries = await Promise.all(
            listed.map(async (file) => {
              try {
                const diff = await gitCommitFileDiff(cwd, sha, file.relative);
                return [
                  file.relative,
                  {
                    file,
                    binary: diff.binary,
                    tooLarge: diff.tooLarge,
                    original: diff.original,
                    current: diff.current,
                  } satisfies LoadedDiff,
                ] as const;
              } catch {
                return [
                  file.relative,
                  {
                    file,
                    binary: false,
                    tooLarge: false,
                    original: "",
                    current: "",
                  } satisfies LoadedDiff,
                ] as const;
              }
            }),
          );
          if (disposed || current !== generation) return;
          setDiffs(new Map(entries));
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
    };
  }, [cwd, sha]);

  const models = useMemo<UnifiedDiffFileModel[]>(() => {
    if (!files) return [];
    return files.map((file) => {
      const loaded = diffs.get(file.relative);
      const unified =
        loaded && !loaded.binary && !loaded.tooLarge
          ? buildUnifiedFile(loaded.original, loaded.current)
          : null;
      return {
        id: file.relative,
        path: file.path,
        label: file.relative,
        binary: loaded?.binary,
        tooLarge: loaded?.tooLarge,
        emptyMessage:
          loaded == null
            ? "Loading…"
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

  return <UnifiedDiffView files={models} totals={totals} />;
}
