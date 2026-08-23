import { acceptCompletion, completionStatus } from "@codemirror/autocomplete";
import { indentLess, indentMore } from "@codemirror/commands";
import {
  foldGutter,
  foldKeymap,
  getIndentUnit,
  indentUnit,
} from "@codemirror/language";
import {
  Annotation,
  Compartment,
  countColumn,
  EditorSelection,
  Prec,
  StateField,
  Transaction,
  type EditorState,
  type Text,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { AlertCircle, ChevronDown, ChevronUp, RotateCcw } from "lucide-react";
import { minimalSetup } from "codemirror";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  MarkdownViewShell,
  useMarkdownMode,
} from "../chrome/MarkdownModeToggle";
import { formatText } from "../lib/format";
import {
  basename,
  gitFileDiff,
  gitStageContents,
  notifyGitChanged,
  readTextFile,
  subscribeGitChanged,
  writeTextFile,
} from "../lib/fs";
import { syncWatchedMtime, watchFile } from "../lib/fileWatch";
import { displayPath } from "../lib/paths";
import type { EditorNavigation } from "../lib/search";
import { useLockOverscroll } from "../lib/useLockOverscroll";
import { isLightScheme } from "../lib/appearance";
import { useColorScheme } from "../lib/useColorScheme";
import { MarkdownPreview } from "./AgentMarkdown";
import { languageForPath, schemeExtensions } from "./editorChrome";
import { editorAutocomplete } from "./editorAutocomplete";
import { editorMatching, editorTyping, tryExpandEmmet } from "./editorEditing";
import {
  diffActiveChunkIndex,
  diffLineStatsForView,
  diffNavigablePositions,
  diffNavUpdateRelevant,
  diffScrollToChunk,
  editorGit,
  setGitOriginal,
} from "./editorGit";
import { editorLint } from "./editorLint";
import { editorSearch } from "./editorSearch";

type EditorNavigationRequest = EditorNavigation & { token: number };

const editorScheme = new Compartment();

type Props = {
  path: string;
  cwd: string;
  active: boolean;
  showDiff?: boolean;
  navigation?: EditorNavigationRequest | null;
  onDirtyChange: (path: string, dirty: boolean) => void;
  onErrorCountChange?: (path: string, count: number) => void;
  onOpenFile?: (path: string) => void;
};

type LoadState =
  | { status: "loading" }
  | { status: "ready"; content: string }
  | { status: "error"; message: string };

type SaveState =
  | { status: "idle" | "saving" | "saved" }
  | { status: "error"; message: string };

export function FileEditor({
  path,
  cwd,
  active,
  showDiff = false,
  navigation,
  onDirtyChange,
  onErrorCountChange,
  onOpenFile,
}: Props) {
  const [loadState, setLoadState] = useState<LoadState>({ status: "loading" });
  const [saveState, setSaveState] = useState<SaveState>({ status: "idle" });
  const [reloadKey, setReloadKey] = useState(0);
  const [draft, setDraft] = useState("");
  const [gitBase, setGitBase] = useState<{
    path: string;
    original: string | null;
  }>({ path, original: null });
  const markdown = isMarkdownPath(path);
  const [mode, setMode] = useMarkdownMode(path);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const saveGeneration = useRef(0);
  const loadGeneration = useRef(0);
  const dirtyRef = useRef(false);
  const pendingDiskRef = useRef(false);

  const applyDiskContent = useCallback((content: string) => {
    setLoadState((current) => {
      if (current.status === "ready" && current.content === content) {
        return current;
      }
      return { status: "ready", content };
    });
    setDraft(content);
  }, []);

  const reloadFromDisk = useCallback(async () => {
    const generation = ++loadGeneration.current;
    try {
      const content = await readTextFile(path);
      if (generation !== loadGeneration.current) return;
      if (dirtyRef.current) {
        pendingDiskRef.current = true;
        return;
      }
      pendingDiskRef.current = false;
      applyDiskContent(content);
    } catch (error: unknown) {
      if (generation !== loadGeneration.current) return;
      if (dirtyRef.current) {
        pendingDiskRef.current = true;
        return;
      }
      setLoadState({
        status: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, [applyDiskContent, path]);

  useEffect(() => {
    dirtyRef.current = false;
    pendingDiskRef.current = false;
    let cancelled = false;
    setLoadState({ status: "loading" });
    setSaveState({ status: "idle" });
    const generation = ++loadGeneration.current;
    void readTextFile(path)
      .then((content) => {
        if (cancelled || generation !== loadGeneration.current) return;
        setLoadState({ status: "ready", content });
        setDraft(content);
      })
      .catch((error: unknown) => {
        if (cancelled || generation !== loadGeneration.current) return;
        setLoadState({
          status: "error",
          message: error instanceof Error ? error.message : String(error),
        });
      });
    return () => {
      cancelled = true;
    };
  }, [path, reloadKey]);

  useEffect(() => {
    if (!showDiff) {
      setGitBase({ path, original: null });
      return;
    }
    const relative = displayPath(path, cwd);
    if (!cwd || cwd === "~" || !relative || relative === path) {
      setGitBase({ path, original: null });
      return;
    }
    let cancelled = false;
    setGitBase({ path, original: null });

    const load = () => {
      void gitFileDiff(cwd, relative)
        .then((diff) => {
          if (cancelled) return;
          if (diff.binary || diff.tooLarge) {
            setGitBase({ path, original: null });
            return;
          }
          setGitBase({ path, original: diff.original });
        })
        .catch(() => {
          if (!cancelled) setGitBase({ path, original: null });
        });
    };

    load();
    const onResume = () => load();
    window.addEventListener("focus", onResume);
    const unsubGit = subscribeGitChanged(onResume);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", onResume);
      unsubGit();
    };
  }, [cwd, path, showDiff]);

  const gitOriginal = gitBase.path === path ? gitBase.original : null;

  useEffect(() => {
    if (loadState.status !== "ready") return;
    let timer = 0;
    const stop = watchFile(path, () => {
      if (dirtyRef.current) {
        pendingDiskRef.current = true;
        return;
      }
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void reloadFromDisk();
      }, 50);
    });
    return () => {
      window.clearTimeout(timer);
      stop();
    };
  }, [loadState.status, path, reloadFromDisk]);

  const save = useCallback(
    async (content: string) => {
      const generation = ++saveGeneration.current;
      setSaveState({ status: "saving" });
      const operation = saveQueue.current.then(() =>
        writeTextFile(path, content),
      );
      saveQueue.current = operation.catch(() => {});
      try {
        await operation;
        await syncWatchedMtime(path);
        notifyGitChanged();
        if (generation === saveGeneration.current) {
          setSaveState({ status: "saved" });
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (generation === saveGeneration.current) {
          setSaveState({ status: "error", message });
        }
        throw error;
      }
    },
    [path],
  );

  const stageGit = useCallback(
    async (contents: string) => {
      const relative = displayPath(path, cwd);
      if (!cwd || cwd === "~" || !relative || relative === path) {
        throw new Error("Can't stage this file");
      }
      try {
        await gitStageContents(cwd, relative, contents);
        notifyGitChanged();
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setSaveState({ status: "error", message });
        throw error;
      }
    },
    [cwd, path],
  );

  const dirtyChange = useCallback(
    (dirty: boolean) => {
      dirtyRef.current = dirty;
      onDirtyChange(path, dirty);
      if (dirty) {
        setSaveState((current) =>
          current.status === "saving" ? current : { status: "idle" },
        );
        return;
      }
      if (pendingDiskRef.current) {
        pendingDiskRef.current = false;
        void reloadFromDisk();
      }
    },
    [onDirtyChange, path, reloadFromDisk],
  );

  const errorCountChange = useCallback(
    (count: number) => onErrorCountChange?.(path, count),
    [onErrorCountChange, path],
  );

  const relativePath = path.startsWith(`${cwd}/`)
    ? path.slice(cwd.length + 1)
    : path;

  if (loadState.status === "loading") {
    return (
      <div className="grid h-full place-items-center text-[12px] text-content/45">
        Opening {basename(path)}…
      </div>
    );
  }

  if (loadState.status === "error") {
    return (
      <div className="grid h-full place-items-center p-6">
        <div className="max-w-md text-center">
          <AlertCircle className="mx-auto mb-3 size-5 text-red-400" />
          <p className="text-[13px] text-content">
            Couldn’t open {basename(path)}
          </p>
          <p className="mt-1 text-[12px] leading-5 text-content/50">
            {loadState.message}
          </p>
          <button
            type="button"
            onClick={() => setReloadKey((value) => value + 1)}
            className="mx-auto mt-4 flex h-7 items-center gap-1.5 rounded-md bg-content/10 px-2.5 text-[12px] text-content hover:bg-content/15"
          >
            <RotateCcw className="size-3" strokeWidth={1.75} />
            Retry
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      {markdown ? (
        <MarkdownViewShell
          mode={mode}
          onModeChange={setMode}
          preview={
            <MarkdownPreview text={draft} cwd={cwd} onOpenFile={onOpenFile} />
          }
          source={
            <div className="flex h-full min-h-0 min-w-0 flex-col">
              <CodeMirrorEditor
                key={`${path}:${reloadKey}`}
                path={path}
                value={loadState.content}
                showDiff={showDiff}
                gitOriginal={gitOriginal}
                active={active && mode === "source"}
                navigation={navigation}
                onDirtyChange={dirtyChange}
                onErrorCountChange={errorCountChange}
                onSave={save}
                onStageGit={showDiff ? stageGit : undefined}
                onDocChange={setDraft}
              />
            </div>
          }
        />
      ) : (
        <CodeMirrorEditor
          key={`${path}:${reloadKey}`}
          path={path}
          value={loadState.content}
          showDiff={showDiff}
          gitOriginal={gitOriginal}
          active={active}
          navigation={navigation}
          onDirtyChange={dirtyChange}
          onErrorCountChange={errorCountChange}
          onSave={save}
          onStageGit={showDiff ? stageGit : undefined}
        />
      )}
      <footer className="flex h-6 shrink-0 items-center border-t border-content/10 px-2.5 font-mono text-[10.5px] text-content/40">
        <span className="min-w-0 flex-1 truncate" title={path}>
          {relativePath}
        </span>
        {saveState.status === "saving" ? (
          <span>Saving…</span>
        ) : saveState.status === "saved" ? (
          <span>Saved</span>
        ) : saveState.status === "error" ? (
          <span
            className="max-w-64 truncate text-red-400"
            title={saveState.message}
          >
            Save failed: {saveState.message}
          </span>
        ) : null}
      </footer>
    </div>
  );
}

function CodeMirrorEditor({
  path,
  value,
  showDiff,
  gitOriginal,
  active,
  navigation,
  onDirtyChange,
  onErrorCountChange,
  onSave,
  onStageGit,
  onDocChange,
}: {
  path: string;
  value: string;
  showDiff: boolean;
  gitOriginal: string | null;
  active: boolean;
  navigation?: EditorNavigationRequest | null;
  onDirtyChange: (dirty: boolean) => void;
  onErrorCountChange: (count: number) => void;
  onSave: (content: string) => Promise<void>;
  onStageGit?: (contents: string) => Promise<void>;
  onDocChange?: (content: string) => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const savedDocumentRef = useRef<Text | null>(null);
  const dirtyRef = useRef(false);
  const activeRef = useRef(active);
  const onDirtyChangeRef = useRef(onDirtyChange);
  const onErrorCountChangeRef = useRef(onErrorCountChange);
  const onSaveRef = useRef(onSave);
  const onStageGitRef = useRef(onStageGit);
  const onDocChangeRef = useRef(onDocChange);
  const valueRef = useRef(value);
  const gitOriginalRef = useRef(gitOriginal);
  const chunkNavPinnedRef = useRef<number | null>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const colorScheme = useColorScheme();
  const [chunkNav, setChunkNav] = useState<{
    positions: number[];
    index: number;
    additions: number;
    deletions: number;
  } | null>(null);
  activeRef.current = active;
  onDirtyChangeRef.current = onDirtyChange;
  onErrorCountChangeRef.current = onErrorCountChange;
  onSaveRef.current = onSave;
  onStageGitRef.current = onStageGit;
  onDocChangeRef.current = onDocChange;
  valueRef.current = value;
  gitOriginalRef.current = gitOriginal;

  const syncChunkNav = useCallback((view: EditorView, fromScroll = true) => {
    const positions = diffNavigablePositions(view);
    const { additions, deletions } = diffLineStatsForView(view);
    if (positions.length === 0) {
      setChunkNav(null);
      chunkNavPinnedRef.current = null;
      return;
    }
    let index = chunkNavPinnedRef.current;
    if (
      fromScroll ||
      index === null ||
      index < 0 ||
      index >= positions.length
    ) {
      index = diffActiveChunkIndex(view, positions);
    }
    chunkNavPinnedRef.current = index;
    setChunkNav({ positions, index, additions, deletions });
  }, []);

  const stepChunkNav = useCallback(
    (delta: number) => {
      const view = viewRef.current;
      if (!view || !chunkNav) return;
      const next = Math.min(
        chunkNav.positions.length - 1,
        Math.max(0, chunkNav.index + delta),
      );
      if (next === chunkNav.index) return;
      chunkNavPinnedRef.current = next;
      diffScrollToChunk(view, chunkNav.positions[next]);
      setChunkNav({
        positions: chunkNav.positions,
        index: next,
        additions: chunkNav.additions,
        deletions: chunkNav.deletions,
      });
    },
    [chunkNav],
  );

  const setDirty = (nextDirty: boolean) => {
    if (dirtyRef.current === nextDirty) return;
    dirtyRef.current = nextDirty;
    onDirtyChangeRef.current(nextDirty);
  };

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const language = new Compartment();
    let disposed = false;
    let saveGeneration = 0;
    let view: EditorView;

    const markDirty = () => {
      const saved = savedDocumentRef.current;
      setDirty(saved ? !view.state.doc.eq(saved) : false);
    };

    const save = () => {
      const generation = ++saveGeneration;
      void (async () => {
        const before = view.state.doc.toString();
        const result = await formatText(
          path,
          before,
          view.state.selection.main.head,
        );
        if (disposed || generation !== saveGeneration) return;

        if (
          result &&
          result.formatted !== before &&
          view.state.doc.toString() === before
        ) {
          const { scrollTop, scrollLeft } = view.scrollDOM;
          view.dispatch({
            changes: {
              from: 0,
              to: view.state.doc.length,
              insert: result.formatted,
            },
            selection: {
              anchor: Math.min(
                Math.max(0, result.cursorOffset),
                result.formatted.length,
              ),
            },
            scrollIntoView: false,
          });
          const restoreScroll = () => {
            if (disposed) return;
            view.scrollDOM.scrollTop = scrollTop;
            view.scrollDOM.scrollLeft = scrollLeft;
          };
          restoreScroll();
          requestAnimationFrame(restoreScroll);
        }

        const document = view.state.doc;
        try {
          await onSaveRef.current(document.toString());
        } catch {
          return;
        }
        if (disposed || generation !== saveGeneration) return;
        savedDocumentRef.current = document;
        markDirty();
      })();
      return true;
    };

    view = new EditorView({
      doc: valueRef.current,
      parent: host,
      extensions: [
        minimalSetup,
        showDiff
          ? editorGit(
              onStageGit
                ? {
                    onStage: (contents) => onStageGitRef.current?.(contents),
                  }
                : undefined,
            )
          : [],
        lineNumbers(),
        foldGutter(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        EditorView.lineWrapping,
        wrappedLineIndent,
        language.of([]),
        editorScheme.of(schemeExtensions(isLightScheme() ? "light" : "dark")),
        editorMatching,
        editorTyping(path),
        editorAutocomplete,
        editorLint(path, (count) => onErrorCountChangeRef.current(count)),
        editorSearch,
        Prec.high(
          keymap.of([
            ...foldKeymap,
            { key: "Mod-s", run: save, preventDefault: true },
            {
              key: "Tab",
              run: (view) => {
                if (
                  completionStatus(view.state) === "active" &&
                  acceptCompletion(view)
                ) {
                  return true;
                }
                return tryExpandEmmet(view) || indentOrInsertTab(view);
              },
              shift: indentLess,
              preventDefault: true,
            },
          ]),
        ),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          onDocChangeRef.current?.(update.state.doc.toString());
          if (update.transactions.some((tr) => tr.annotation(diskReload))) {
            return;
          }
          markDirty();
        }),
        showDiff
          ? EditorView.updateListener.of((update) => {
              if (!diffNavUpdateRelevant(update)) return;
              chunkNavPinnedRef.current = null;
              syncChunkNav(update.view);
            })
          : [],
      ],
    });
    savedDocumentRef.current = view.state.doc;
    dirtyRef.current = false;
    viewRef.current = view;
    lockOverscroll(view.scrollDOM as HTMLDivElement);
    if (showDiff) {
      if (gitOriginalRef.current) {
        setGitOriginal(view, gitOriginalRef.current);
      }
      syncChunkNav(view);
    } else {
      setChunkNav(null);
    }
    if (activeRef.current) view.focus();

    void languageForPath(path).then((extension) => {
      if (!disposed && extension) {
        view.dispatch({ effects: language.reconfigure(extension) });
      }
    });

    return () => {
      disposed = true;
      onErrorCountChangeRef.current(0);
      lockOverscroll(null);
      viewRef.current = null;
      savedDocumentRef.current = null;
      setChunkNav(null);
      view.destroy();
    };
  }, [lockOverscroll, path, showDiff, syncChunkNav]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: editorScheme.reconfigure(schemeExtensions(colorScheme)),
    });
  }, [colorScheme]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !showDiff) return;
    setGitOriginal(view, gitOriginal);
    chunkNavPinnedRef.current = null;
    syncChunkNav(view);
  }, [gitOriginal, showDiff, syncChunkNav]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !showDiff) return;
    const onScroll = () => {
      chunkNavPinnedRef.current = null;
      syncChunkNav(view);
    };
    view.scrollDOM.addEventListener("scroll", onScroll, { passive: true });
    return () => view.scrollDOM.removeEventListener("scroll", onScroll);
  }, [showDiff, syncChunkNav, path]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || dirtyRef.current) return;
    if (view.state.doc.toString() === value) return;
    const { scrollTop, scrollLeft } = view.scrollDOM;
    const selection = view.state.selection.main;
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: value,
      },
      selection: {
        anchor: Math.min(selection.anchor, value.length),
        head: Math.min(selection.head, value.length),
      },
      annotations: [Transaction.addToHistory.of(false), diskReload.of(true)],
      scrollIntoView: false,
    });
    view.scrollDOM.scrollTop = scrollTop;
    view.scrollDOM.scrollLeft = scrollLeft;
    savedDocumentRef.current = view.state.doc;
    setDirty(false);
  }, [value]);

  useEffect(() => {
    if (!navigation) return;
    const view = viewRef.current;
    if (!view) return;

    let cancelled = false;
    const run = () => {
      if (cancelled) return;
      if (view.state.doc.lines < navigation.line) {
        requestAnimationFrame(run);
        return;
      }
      if (cancelled) return;
      revealNavigation(view, navigation);
    };
    requestAnimationFrame(() => requestAnimationFrame(run));

    return () => {
      cancelled = true;
    };
  }, [navigation]);

  useEffect(() => {
    if (!active) return;
    const view = viewRef.current;
    if (!view) return;
    view.requestMeasure();
    const activeEl = document.activeElement;
    if (
      activeEl &&
      view.dom.contains(activeEl) &&
      activeEl !== view.contentDOM
    ) {
      return;
    }
    view.focus();
  }, [active, path]);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {showDiff && chunkNav ? (
        <DiffChunkNav
          index={chunkNav.index}
          total={chunkNav.positions.length}
          additions={chunkNav.additions}
          deletions={chunkNav.deletions}
          onPrev={() => stepChunkNav(-1)}
          onNext={() => stepChunkNav(1)}
        />
      ) : null}
      <div ref={hostRef} className="min-h-0 flex-1" />
    </div>
  );
}

function DiffChunkNav({
  index,
  total,
  additions,
  deletions,
  onPrev,
  onNext,
}: {
  index: number;
  total: number;
  additions: number;
  deletions: number;
  onPrev: () => void;
  onNext: () => void;
}) {
  return (
    <header
      className="flex h-8 shrink-0 items-center justify-between gap-3 border-b border-content/10 px-3 pr-1"
      role="toolbar"
      aria-label="Jump between changes"
    >
      <DiffChunkStat additions={additions} deletions={deletions} />
      <div className="flex items-center gap-0.5">
        <button
          type="button"
          title="Previous change"
          aria-label="Previous change"
          disabled={index <= 0}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onPrev}
          className="grid size-6 place-items-center rounded text-content/70 hover:bg-content/10 hover:text-content disabled:opacity-35"
        >
          <ChevronUp className="size-3.5" strokeWidth={1.75} />
        </button>
        <span className="min-w-10 px-0.5 text-center font-mono text-[10.5px] font-medium tabular-nums text-content/55 select-none">
          {index + 1}/{total}
        </span>
        <button
          type="button"
          title="Next change"
          aria-label="Next change"
          disabled={index >= total - 1}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onNext}
          className="grid size-6 place-items-center rounded text-content/70 hover:bg-content/10 hover:text-content disabled:opacity-35"
        >
          <ChevronDown className="size-3.5" strokeWidth={1.75} />
        </button>
      </div>
    </header>
  );
}

function DiffChunkStat({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  if (additions <= 0 && deletions <= 0) {
    return <span className="min-w-0 flex-1" />;
  }
  return (
    <span className="flex min-w-0 shrink-0 items-center gap-1.5 font-mono text-[11px] font-semibold tabular-nums">
      {additions > 0 ? (
        <span className="text-emerald-400">+{additions}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{deletions}</span>
      ) : null}
    </span>
  );
}

function revealNavigation(view: EditorView, target: EditorNavigation) {
  const lineNumber = Math.min(Math.max(1, target.line), view.state.doc.lines);
  const line = view.state.doc.line(lineNumber);
  const column = Math.max(1, target.column ?? 1);
  const anchor = Math.min(line.from + column - 1, line.to);
  view.dispatch({
    selection: { anchor },
    effects: EditorView.scrollIntoView(anchor, { y: "center" }),
  });
  view.focus();
}

const diskReload = Annotation.define<boolean>();

function indentOrInsertTab(view: EditorView): boolean {
  const { state, dispatch } = view;
  if (state.readOnly) return false;
  if (state.selection.ranges.some((range) => !range.empty)) {
    return indentMore(view);
  }

  const unit = state.facet(indentUnit);
  if (unit === "\t") {
    dispatch(
      state.update(state.replaceSelection("\t"), {
        scrollIntoView: true,
        userEvent: "input",
      }),
    );
    return true;
  }

  const width = getIndentUnit(state);
  dispatch(
    state.update(
      state.changeByRange((range) => {
        const line = state.doc.lineAt(range.head);
        const column = countColumn(
          line.text.slice(0, range.head - line.from),
          state.tabSize,
        );
        const insert = " ".repeat(width - (column % width) || width);
        return {
          changes: { from: range.head, insert },
          range: EditorSelection.cursor(range.head + insert.length),
        };
      }),
      { scrollIntoView: true, userEvent: "input" },
    ),
  );
  return true;
}

type LineRange = { from: number; to: number };

const wrappedLineIndent = StateField.define<DecorationSet>({
  create(state) {
    return Decoration.set(
      indentDecorations(state, [{ from: 0, to: state.doc.length }]),
      true,
    );
  },
  update(decorations, transaction) {
    if (!transaction.docChanged) return decorations;
    const ranges: LineRange[] = [];
    transaction.changes.iterChangedRanges((_fromA, _toA, fromB, toB) => {
      const from = transaction.state.doc.lineAt(fromB).from;
      const to = transaction.state.doc.lineAt(toB).to;
      ranges.push({ from, to });
    });
    const changedLines = mergeLineRanges(ranges);
    return decorations.map(transaction.changes).update({
      filter: (from) =>
        !changedLines.some((range) => from >= range.from && from <= range.to),
      add: indentDecorations(transaction.state, changedLines),
      sort: true,
    });
  },
  provide: (field) => EditorView.decorations.from(field),
});

function indentDecorations(state: EditorState, ranges: LineRange[]) {
  const decorations = [];
  for (const range of ranges) {
    let line = state.doc.lineAt(range.from);
    while (line.from <= range.to) {
      const columns = leadingIndentColumns(line.text, state.tabSize);
      if (columns > 0) {
        const indent = Math.min(columns, 40);
        decorations.push(
          Decoration.line({
            attributes: {
              class: "cm-wrapped-indent",
              style: `padding-left: calc(${indent}ch + 6px); text-indent: -${indent}ch`,
            },
          }).range(line.from),
        );
      }
      if (line.number >= state.doc.lines) break;
      line = state.doc.line(line.number + 1);
    }
  }
  return decorations;
}

function leadingIndentColumns(text: string, tabSize: number): number {
  let columns = 0;
  for (const character of text) {
    if (character === " ") {
      columns += 1;
    } else if (character === "\t") {
      columns += tabSize - (columns % tabSize);
    } else {
      break;
    }
  }
  return columns;
}

function mergeLineRanges(ranges: LineRange[]): LineRange[] {
  const sorted = [...ranges].sort((a, b) => a.from - b.from);
  const merged: LineRange[] = [];
  for (const range of sorted) {
    const previous = merged[merged.length - 1];
    if (previous && range.from <= previous.to + 1) {
      previous.to = Math.max(previous.to, range.to);
    } else {
      merged.push({ ...range });
    }
  }
  return merged;
}

function isMarkdownPath(path: string): boolean {
  const name = basename(path).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";
  return [".md", ".mdx", ".markdown"].includes(extension);
}
