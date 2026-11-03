import { Profiler, useState } from "react";
import { createRoot } from "react-dom/client";
import { buildUnifiedFile } from "../../src/features/source-control/model/unifiedDiff";
import {
  applyThemePreference,
  saveThemePreference,
} from "../../src/features/settings/model/appearance";
import {
  UnifiedDiffView,
  type UnifiedDiffFileModel,
} from "../../src/features/source-control/ui/UnifiedDiffView";
import "../../src/styles/index.css";

const params = new URLSearchParams(location.search);
const theme = params.has("light") ? "light" : "dark";
saveThemePreference(theme);
applyThemePreference(theme);
const metrics = {
  commits: 0,
  renderMs: 0,
  lastCommitAt: 0,
  stages: [] as { id: string; pos?: number }[],
};
const files: UnifiedDiffFileModel[] = Array.from({ length: 48 }, (_, index) => {
  const original = Array.from(
    { length: 420 },
    (_, line) =>
      `export const value${line} = ${line}; // ${index === 0 && line === 14 ? "long context ".repeat(80) : "review context"}`,
  );
  const current = original.map((text, line) =>
    [12, 120, 310].includes(line)
      ? text.replace(`= ${line};`, `= ${line + 1};`)
      : text,
  );
  if (index === 0)
    current[13] =
      "\tconst greeting = '日本語 and a long line that should remain selectable';";
  const diff = buildUnifiedFile(original.join("\n"), current.join("\n"));
  return {
    id: `file-${index}`,
    path: `/repo/src/review/file-${index}.ts`,
    label: `src/review/file-${index}.ts`,
    additions: diff.additions,
    deletions: diff.deletions,
    blocks: diff.blocks,
    canStage: true,
    canDiscard: true,
    canStageHunk: true,
  };
});

function Fixture() {
  const [models, setModels] = useState(files);
  const [focusId, setFocusId] = useState<string>();
  const [visible, setVisible] = useState(true);
  Object.assign(window, {
    useLightDiffTheme: () => {
      saveThemePreference("light");
      applyThemePreference("light");
    },
    setDiffVisible: setVisible,
    diffMetrics: () => ({
      ...metrics,
      nodes: document.getElementsByTagName("*").length,
    }),
    resetDiffMetrics: () => {
      metrics.commits = 0;
      metrics.renderMs = 0;
    },
    focusDiffFile: (index: number) => setFocusId(`file-${index}`),
    refreshDiff: () =>
      setModels((current) => current.map((file) => ({ ...file }))),
    removeDiffFile: (id: string) =>
      setModels((current) => current.filter((file) => file.id !== id)),
    expectedFirstStage: () =>
      files[0].blocks
        .flatMap((block) => block.lines)
        .find((line) => line.kind === "add")?.pos,
    growFirstDiff: () =>
      setModels((current) => {
        const diff = buildUnifiedFile(
          "",
          Array.from(
            { length: 500 },
            (_, index) => `const growing${index} = ${index};`,
          ).join("\n"),
        );
        return current.map((file, index) =>
          index === 0
            ? {
                ...file,
                blocks: diff.blocks,
                additions: diff.additions,
                deletions: 0,
              }
            : file,
        );
      }),
    useManyDiffFiles: () =>
      setModels(
        Array.from({ length: 500 }, (_, index) => ({
          ...files[index % files.length],
          id: `many-${index}`,
        })),
      ),
    useHugeDiffFile: () => {
      const text = Array.from(
        { length: 20_000 },
        (_, index) => `const large${index} = ${index};`,
      ).join("\n");
      const diff = buildUnifiedFile("", text);
      setModels([
        {
          ...files[0],
          id: "huge",
          blocks: diff.blocks,
          additions: diff.additions,
          deletions: 0,
        },
      ]);
    },
  });
  return (
    <Profiler
      id="diff"
      onRender={(_id, _phase, duration) => {
        metrics.commits++;
        metrics.renderMs += duration;
        metrics.lastCommitAt = performance.now();
      }}
    >
      <div
        style={{
          height: "100dvh",
          display: "flex",
          background: "var(--color-background-base)",
        }}
      >
        <div
          style={{
            visibility: visible ? "visible" : "hidden",
            flex: 1,
            minWidth: 0,
            ...(params.has("embedded")
              ? { overflow: "auto" as const }
              : { display: "flex" }),
          }}
          data-diff-fixture-scroll
        >
          {params.has("embedded") ? (
            <div style={{ height: 220 }}>
              Review context above the embedded diff
            </div>
          ) : null}
          <UnifiedDiffView
            files={models}
            visible={visible}
            focusId={focusId}
            fill={!params.has("embedded")}
            fileLayout={params.has("embedded") ? "cards" : "stacked"}
            onStageFile={(id) => metrics.stages.push({ id })}
            onStageHunk={(id, pos) => metrics.stages.push({ id, pos })}
            onDiscardFile={(id) => metrics.stages.push({ id: `discard-${id}` })}
          />
        </div>
      </div>
    </Profiler>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
