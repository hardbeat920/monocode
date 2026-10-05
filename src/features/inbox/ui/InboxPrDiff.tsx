import { useCallback, useMemo, useRef, useState } from "react";
import type { GithubPrDiff } from "../model/githubTasks";
import {
  mergePrDiff,
  parsePrPatch,
  type PrDiffFile,
} from "../../source-control/model/prDiff";
import {
  blocksFromLines,
  buildUnifiedFile,
  type UnifiedLine,
} from "../../source-control/model/unifiedDiff";
import {
  UnifiedDiffView,
  type UnifiedDiffFileModel,
} from "../../source-control/ui/UnifiedDiffView";

type Props = {
  diff: GithubPrDiff;
  /** When true, show the whole file (no fold rows). */
  fullFile?: boolean;
  /** File to open and scroll to, such as one picked from the summary. */
  focusPath?: string;
  loadFile?: (path: string) => Promise<InboxDiffContents>;
};

export type InboxDiffContents = {
  original: string;
  current: string;
  binary: boolean;
  tooLarge: boolean;
};

type FileLoad = {
  status: "loading" | "loaded" | "error";
  model: UnifiedDiffFileModel;
};

export function InboxPrDiff({
  diff,
  fullFile = false,
  focusPath,
  loadFile,
}: Props) {
  const preview = useMemo(() => {
    const context = fullFile ? Number.POSITIVE_INFINITY : undefined;
    const models = mergePrDiff(diff.files, parsePrPatch(diff.patch)).map(
      (file) => toModel(file, diff.truncated, context),
    );
    return {
      models: new Map(models.map((model) => [model.id, model])),
      context,
      loadFile,
    };
  }, [diff, fullFile, loadFile]);
  const [loads, setLoads] = useState(() => ({
    preview,
    files: new Map<string, FileLoad>(),
  }));
  // Reset with the preview, before children can request its files.
  if (loads.preview !== preview) {
    setLoads({ preview, files: new Map() });
  }
  const currentLoads = useRef(loads);
  currentLoads.current = loads;
  const onLoadFile = useCallback(
    (path: string) => {
      const current = currentLoads.current;
      const file = preview.models.get(path);
      const cached = current.files.get(path);
      if (
        current.preview !== preview ||
        !preview.loadFile ||
        !file ||
        file.blocks.length ||
        file.binary ||
        (cached && cached.status !== "error")
      )
        return;
      const update = (entry: FileLoad) => {
        const current = currentLoads.current;
        if (current.preview !== preview) return;
        const next = {
          preview,
          files: new Map(current.files).set(path, entry),
        };
        // Deduplicate calls even before React commits the state update.
        currentLoads.current = next;
        setLoads(next);
      };
      update({
        status: "loading",
        model: { ...file, emptyMessage: "Loading diff…" },
      });
      void preview
        .loadFile(path)
        .then((contents) => {
          if (currentLoads.current.preview !== preview) return;
          update({
            status: "loaded",
            model: {
              ...file,
              binary: contents.binary,
              tooLarge: contents.tooLarge,
              emptyMessage: undefined,
              blocks:
                contents.binary || contents.tooLarge
                  ? []
                  : buildUnifiedFile(
                      contents.original,
                      contents.current,
                      preview.context,
                    ).blocks,
            },
          });
        })
        .catch((error: unknown) => {
          update({
            status: "error",
            model: {
              ...file,
              emptyMessage: `${error instanceof Error ? error.message : String(error)}. Collapse and expand to retry.`,
            },
          });
        });
    },
    [preview],
  );

  const files = useMemo(
    () =>
      Array.from(
        preview.models.values(),
        (file) => loads.files.get(file.id)?.model ?? file,
      ),
    [preview, loads],
  );

  return (
    <UnifiedDiffView
      files={files}
      truncated={diff.truncated}
      totals={{ additions: diff.additions, deletions: diff.deletions }}
      fill={false}
      fileLayout="cards"
      initialExpansion="first"
      focusPath={focusPath}
      onLoadFile={loadFile ? onLoadFile : undefined}
    />
  );
}

function toModel(
  file: PrDiffFile,
  truncated: boolean,
  context?: number,
): UnifiedDiffFileModel {
  const lines = file.lines.map(toUnifiedLine);
  return {
    id: file.path,
    path: file.path,
    label:
      file.status === "renamed" && file.previousPath
        ? `${file.previousPath} → ${file.path}`
        : file.path,
    binary: file.binary,
    emptyMessage:
      !file.binary && file.lines.length === 0
        ? truncated
          ? "Patch unavailable because this change is too large"
          : "No textual diff"
        : undefined,
    additions: file.additions,
    deletions: file.deletions,
    blocks: blocksFromLines(lines, context),
  };
}

function toUnifiedLine(line: PrDiffFile["lines"][number]): UnifiedLine {
  return {
    kind: line.kind,
    text: line.text,
    oldNumber: line.oldNumber,
    newNumber: line.newNumber,
  };
}
