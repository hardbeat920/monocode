import { QuickWorkspaceControls } from "./QuickWorkspaceControls";
import {
  workspaceForProject,
  quickWorkspaceLaunch,
  type QuickWorkspace,
} from "../model/quickWorkspace";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { prettyParent, projectName } from "../../../shared/lib/paths";
import {
  ChevronDown,
  Search,
  Plus,
  X,
  ImagePlus,
  Maximize2,
  Terminal,
} from "../../../shared/ui/icons";
import {
  QuickProjectIcon,
  loadQuickProjectAppearance,
} from "./QuickProjectIcon";
import {
  getModelSnapshot,
  subscribeModels,
  mergeModelSettings,
  loadLastModelSettings,
  saveLastModelSettings,
  saveRecentModelChoice,
} from "../../sessions/model/models";
import {
  DEFAULT_RUNTIME_MODE,
  HARNESS_TITLE,
  type HarnessId,
  type RuntimeMode,
  harnessSupportsAttachments,
} from "../../sessions/model/session";
import { Popover } from "../../../shared/ui/Popover";
import { AttachmentChip } from "../../sessions/ui/AttachmentChip";
import { quickLaunchAttachments } from "../model/quickAttachments";
import { useQuickAttachments } from "./useQuickAttachments";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import { QuickModelSelector } from "./QuickModelSelector";
import { TerminalView } from "../../terminal/ui/TerminalView";
import { useQuickPickerMotion } from "./useQuickPickerMotion";
import {
  applyQuickCatalog,
  filterQuickProjects,
  initialQuickChoice,
  initialQuickProject,
  loadQuickProjects,
  QUICK_COMPOSER_CATALOG_EVENT,
  QUICK_COMPOSER_CATALOG_REQUEST_EVENT,
  QUICK_COMPOSER_SHOWN_EVENT,
  rememberQuickProject,
  resolveQuickModel,
  type QuickLaunch,
} from "../model/quickComposer";

/** Tallest the prompt grows before it scrolls, in px. */
const PROMPT_MAX_HEIGHT = 220;

export function QuickComposer({ onShown }: { onShown: () => void }) {
  const [projects, setProjects] = useState(loadQuickProjects);
  const [projectAppearance, setProjectAppearance] = useState(
    loadQuickProjectAppearance,
  );
  const [availableHarnesses, setAvailableHarnesses] = useState<
    HarnessId[] | null
  >(null);
  const [cwd, setCwd] = useState(() => initialQuickProject(projects));
  const [choice, setChoice] = useState(initialQuickChoice);
  const [workspaceChoice, setWorkspaceChoice] = useState<QuickWorkspace>({
    cwd,
    mode: "current",
  });
  const workspace = workspaceForProject(workspaceChoice, cwd);
  const [gitOpen, setGitOpen] = useState(false);
  // Re-render when a workspace window's live catalog lands.
  const catalogVersion = useSyncExternalStore(
    subscribeModels,
    getModelSnapshot,
  );
  const [modelSettings, setModelSettings] = useState(loadLastModelSettings);
  const [runtimeMode, setRuntimeMode] =
    useState<RuntimeMode>(DEFAULT_RUNTIME_MODE);
  const [prompt, setPrompt] = useState("");
  const [picker, setPicker] = useState<
    "project" | "model" | "attachments" | "terminal" | null
  >(null);
  const [terminalStarted, setTerminalStarted] = useState(false);
  const terminalId = useRef(`quick-composer:${crypto.randomUUID()}`);
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const attachmentsSupported = harnessSupportsAttachments(choice.harness);
  const attachments = useQuickAttachments(
    attachmentsSupported && !busy,
    setError,
  );
  const frameRef = useRef<HTMLDivElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const plusRef = useRef<HTMLButtonElement>(null);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const queryRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const focusPrompt = useCallback(() => {
    requestAnimationFrame(() => {
      const field = promptRef.current;
      if (!field) return;
      field.focus({ preventScroll: true });
      field.setSelectionRange(field.value.length, field.value.length);
    });
  }, []);

  // Projects and defaults can change in the workspace between shows, so each
  // show re-reads them. The draft survives a dismiss, like Spotlight's query.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen(QUICK_COMPOSER_SHOWN_EVENT, () => {
      onShown();
      const nextProjects = loadQuickProjects();
      const nextChoice = initialQuickChoice();
      setProjects(nextProjects);
      setProjectAppearance(loadQuickProjectAppearance());

      setCwd((current) =>
        current && nextProjects.includes(current)
          ? current
          : initialQuickProject(nextProjects),
      );
      setChoice(nextChoice);
      setModelSettings(loadLastModelSettings());
      setPicker(null);
      setError(null);
      void emit(QUICK_COMPOSER_CATALOG_REQUEST_EVENT, nextChoice.harness);
      focusPrompt();
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    focusPrompt();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [focusPrompt, onShown]);

  // Model lists come from the CLIs, which only workspace windows talk to.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen(QUICK_COMPOSER_CATALOG_EVENT, (event) => {
      const available = applyQuickCatalog(event.payload);
      if (available) setAvailableHarnesses(available);
    }).then((stop) => {
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
      void emit(
        QUICK_COMPOSER_CATALOG_REQUEST_EVENT,
        initialQuickChoice().harness,
      );
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Scroll only the list. scrollIntoView also scrolls the clipped card/root
  // while the native window is still catching up with the expanded content.
  useLayoutEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (!list || !row) return;
    const listRect = list.getBoundingClientRect();
    const rowRect = row.getBoundingClientRect();
    if (rowRect.top < listRect.top) {
      list.scrollTop += rowRect.top - listRect.top;
    } else if (rowRect.bottom > listRect.bottom) {
      list.scrollTop += rowRect.bottom - listRect.bottom;
    }
  }, [highlight, picker, query, catalogVersion]);

  useLayoutEffect(() => {
    const field = promptRef.current;
    if (!field) return;
    field.style.height = "auto";
    field.style.height = `${Math.min(field.scrollHeight, PROMPT_MAX_HEIGHT)}px`;
  }, [prompt]);

  const onGitOpenChange = useCallback((open: boolean) => {
    setGitOpen(open);
    if (open) setPicker(null);
  }, []);
  useQuickPickerMotion(frameRef, pickerRef, picker);

  const projectOptions = useMemo(
    () => filterQuickProjects(projects, picker === "project" ? query : ""),
    [picker, projects, query],
  );
  const resolvedModel = resolveQuickModel(choice);
  const model = resolvedModel ?? {
    ...choice,
    id: choice.model,
    name: "Loading model…",
  };
  const settings = mergeModelSettings(model, modelSettings);
  const optionCount = projectOptions.length;
  const openPicker = (
    kind: "project" | "model" | "attachments" | "terminal",
  ) => {
    if (picker === kind) {
      closePicker();
      return;
    }
    setPicker(kind);
    if (kind === "terminal") setTerminalStarted(true);
    setQuery("");
    setHighlight(Math.max(0, projects.indexOf(cwd ?? "")));
    if (kind === "project")
      requestAnimationFrame(() =>
        queryRef.current?.focus({ preventScroll: true }),
      );
  };

  const closePicker = () => {
    setPicker(null);
    setQuery("");
    focusPrompt();
  };

  const chooseAt = (index: number) => {
    if (picker === "project") {
      const path = projectOptions[index];
      if (!path) return;
      setCwd(path);
      setWorkspaceChoice({ cwd: path, mode: "current" });
    }
    closePicker();
  };

  const dismiss = () => {
    void invoke("quick_composer_dismiss");
  };

  const submit = async (reveal: boolean) => {
    const text = prompt.trim();
    if (
      (!text && !attachments.files.length) ||
      !cwd ||
      busy ||
      attachments.loading ||
      gitOpen ||
      !resolvedModel ||
      (attachments.files.length > 0 && !attachmentsSupported)
    )
      return;
    setBusy(true);
    setError(null);
    const picked = { harness: model.harness, model: model.id };
    try {
      const request: QuickLaunch = {
        prompt: text,
        cwd,
        ...picked,
        modelSettings: settings,
        runtimeMode,
        attachments: quickLaunchAttachments(attachments.files),
        ...(await quickWorkspaceLaunch(workspace)),
        reveal,
      };
      await invoke("quick_composer_submit", { request });
      rememberQuickProject(cwd);
      saveLastModelSettings(settings);
      saveRecentModelChoice(picked.harness, picked.model);
      setPrompt("");
      attachments.clear();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const onPromptKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (picker) closePicker();
      else dismiss();
      return;
    }
    if (event.key === "Enter" && !event.shiftKey && !event.altKey) {
      event.preventDefault();
      void submit(event.metaKey);
      return;
    }
    if (event.metaKey && event.key.toLowerCase() === "p") {
      event.preventDefault();
      openPicker("project");
      return;
    }
    if (event.metaKey && event.key === ".") {
      event.preventDefault();
      openPicker("model");
    }
  };

  const onQueryKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closePicker();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (optionCount === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      setHighlight((index) => (index + step + optionCount) % optionCount);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      chooseAt(Math.min(highlight, optionCount - 1));
    }
  };

  const canSubmit = Boolean(
    (prompt.trim() || attachments.files.length) &&
    cwd &&
    !busy &&
    !attachments.loading &&
    !gitOpen &&
    resolvedModel &&
    (attachmentsSupported || !attachments.files.length),
  );

  const optionClass = (index: number) =>
    `flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left text-[13px] ${
      index === highlight
        ? "bg-selection-emphasis text-content"
        : "text-content/75"
    }`;

  return (
    // Selectors expand below the toolbar without moving the prompt.
    <div
      ref={frameRef}
      onPaste={(event) => {
        if (
          event.target instanceof Element &&
          event.target.closest(".monocode-terminal")
        )
          return;
        attachments.onPaste(event);
      }}
      onDragOver={attachments.onDragOver}
      onDragLeave={attachments.onDragLeave}
      onDrop={attachments.onDrop}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || event.defaultPrevented) return;
        if (
          event.target instanceof Element &&
          event.target.closest(".monocode-terminal")
        )
          return;
        event.preventDefault();
        if (picker) closePicker();
        else dismiss();
      }}
      className="relative flex max-h-[520px] flex-col overflow-clip rounded-[16px] border border-content/10 bg-background-base/45 text-content"
    >
      <div
        title="Drag to move"
        className="group absolute inset-x-0 top-0 z-10 flex h-3 cursor-grab items-start justify-center pt-1 active:cursor-grabbing"
        onMouseDown={(event) => {
          if (event.button !== 0) return;
          event.preventDefault();
          void getCurrentWindow()
            .startDragging()
            .catch(() => undefined);
        }}
      >
        <span className="pointer-events-none h-0.5 w-6 rounded-full bg-content/15 transition-colors group-hover:bg-content/35" />
      </div>
      <button
        type="button"
        aria-label="Close composer"
        title="Close (Esc)"
        onClick={dismiss}
        className="absolute right-2 top-2 z-20 grid size-5 place-items-center rounded text-content/35 hover:bg-selection-hover hover:text-content"
      >
        <X className="size-3" />
      </button>
      {attachments.dragging ? (
        <div className="pointer-events-none absolute inset-0 z-30 grid place-items-center rounded-[16px] border border-dashed border-accent/60 bg-background-base/90 text-sm text-accent">
          Drop to attach
        </div>
      ) : null}
      <div className="flex shrink-0 items-center px-5 pt-3 pr-9">
        <QuickWorkspaceControls
          key={cwd}
          value={workspace}
          enabled={!busy && !picker}
          onChange={setWorkspaceChoice}
          onError={setError}
          onOpenChange={onGitOpenChange}
          onClose={focusPrompt}
        />
      </div>
      {attachments.files.length ? (
        <div
          aria-label="Attachments"
          className="flex max-h-28 shrink-0 flex-wrap gap-1.5 overflow-y-auto px-5 pt-4 pb-1"
        >
          {attachments.files.map((file) => (
            <AttachmentChip
              key={file.id}
              attachment={file}
              onRemove={busy ? undefined : () => attachments.remove(file.id)}
            />
          ))}
        </div>
      ) : null}
      <textarea
        ref={promptRef}
        value={prompt}
        rows={2}
        onChange={(event) => setPrompt(event.target.value)}
        onKeyDown={onPromptKeyDown}
        placeholder={
          cwd
            ? `Start a ${HARNESS_TITLE[model.harness]} session in ${projectName(cwd)}…`
            : "Open a project in MonoCode first"
        }
        disabled={!cwd}
        aria-label="Prompt"
        spellCheck
        className="block w-full shrink-0 resize-none bg-transparent pl-5 pr-9 pt-4 pb-2 text-[16px] leading-6 text-content outline-none select-text placeholder:text-content/40"
      />

      {attachments.files.length && !attachmentsSupported ? (
        <p role="alert" className="px-5 pb-2 text-xs text-amber-400">
          Choose a provider that supports attachments, or remove the attached
          files.
        </p>
      ) : null}
      <div className="flex shrink-0 items-center gap-1.5 border-t border-stroke px-3 py-2">
        <button
          type="button"
          ref={plusRef}
          aria-label="Add attachment"
          aria-expanded={picker === "attachments"}
          title={
            attachmentsSupported
              ? "Attach files or take a screenshot"
              : "This provider does not support attachments"
          }
          disabled={!attachmentsSupported || attachments.loading || busy}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => openPicker("attachments")}
          className={`grid size-6.5 shrink-0 place-items-center rounded-md disabled:opacity-40 ${picker === "attachments" ? "bg-selection-emphasis text-content" : "text-content/70 hover:bg-selection-hover hover:text-content"}`}
        >
          <Plus className="size-3.5" strokeWidth={1.5} />
        </button>
        <button
          type="button"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => openPicker("project")}
          disabled={projects.length === 0}
          title="Project (⌘P)"
          aria-expanded={picker === "project"}
          className={`flex min-w-0 max-w-[40%] items-center gap-1.5 rounded-md px-2 py-1 text-[12px] disabled:opacity-50 ${picker === "project" ? "bg-selection-emphasis text-content" : "text-content/70 hover:bg-selection-hover hover:text-content"}`}
        >
          {cwd ? (
            <QuickProjectIcon
              projectPath={cwd}
              appearance={projectAppearance}
              className="size-3 shrink-0"
            />
          ) : null}
          <span className="truncate">
            {cwd ? projectName(cwd) : "No project"}
          </span>
          <ChevronDown className="size-3 shrink-0 opacity-60" />
        </button>
        <button
          type="button"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => openPicker("model")}
          title="Model (⌘.)"
          aria-expanded={picker === "model"}
          className={`flex min-w-0 max-w-[40%] items-center gap-1.5 rounded-md px-2 py-1 text-[12px] ${picker === "model" ? "bg-selection-emphasis text-content" : "text-content/70 hover:bg-selection-hover hover:text-content"}`}
        >
          <HarnessIcon harness={model.harness} className="size-3.5 shrink-0" />
          <span className="truncate">{model.name}</span>
          <ChevronDown className="size-3 shrink-0 opacity-60" />
        </button>
        <button
          type="button"
          aria-label="Toggle floating terminal"
          aria-expanded={picker === "terminal"}
          title="Floating terminal"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => openPicker("terminal")}
          className={`grid size-6.5 shrink-0 place-items-center rounded-md ${picker === "terminal" ? "bg-selection-emphasis text-content" : "text-content/70 hover:bg-selection-hover hover:text-content"}`}
        >
          <Terminal className="size-3.5" strokeWidth={1.5} />
        </button>
        <span className="ml-auto flex shrink-0 items-center gap-3 text-[11px] text-content/45">
          {attachments.loading ? (
            <span role="status">Adding attachment…</span>
          ) : error ? (
            <span className="max-w-72 truncate text-red-400" title={error}>
              {error}
            </span>
          ) : (
            <>
              <span>
                <Kbd>↵</Kbd> start
              </span>
              <span>
                <Kbd>⌘↵</Kbd> start and open
              </span>
            </>
          )}
          <button
            type="button"
            onClick={() => void submit(false)}
            disabled={!canSubmit}
            className="rounded-md bg-accent px-2.5 py-1 text-[12px] font-medium text-white disabled:opacity-40"
          >
            Start
          </button>
        </span>
      </div>

      {picker === "attachments" ? (
        <Popover
          anchor={plusRef}
          side="top"
          align="start"
          width={220}
          gap={4}
          autoFocus
          tabIndex={-1}
          onDismiss={closePicker}
          className="p-1"
        >
          <button
            type="button"
            disabled={attachments.loading || !attachmentsSupported}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              setPicker(null);
              void attachments.chooseFiles().then(focusPrompt);
            }}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-selection-hover disabled:opacity-40"
          >
            <ImagePlus className="size-3.5" />
            Choose files…
          </button>
          <button
            type="button"
            disabled={attachments.loading || !attachmentsSupported}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              setPicker(null);
              void attachments.takeScreenshot().then(focusPrompt);
            }}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-selection-hover disabled:opacity-40"
          >
            <Maximize2 className="size-3.5" />
            Take screenshot…
          </button>
        </Popover>
      ) : null}
      {terminalStarted ? (
        <div
          ref={picker === "terminal" ? pickerRef : undefined}
          className={`h-[320px] shrink-0 border-t border-stroke ${picker === "terminal" ? "" : "hidden"}`}
          aria-label="Floating terminal"
        >
          <TerminalView
            id={terminalId.current}
            cwd=""
            active={picker === "terminal"}
          />
        </div>
      ) : null}
      {picker && picker !== "attachments" && picker !== "terminal" ? (
        <div ref={pickerRef} key={picker} className="flex min-h-0 flex-col">
          {picker === "model" ? (
            <QuickModelSelector
              model={model}
              values={settings}
              runtimeMode={runtimeMode}
              onRuntimeModeChange={setRuntimeMode}
              availableHarnesses={availableHarnesses}
              onChange={(selected) => {
                setChoice({ harness: selected.harness, model: selected.id });
                setModelSettings((current) =>
                  mergeModelSettings(selected, current),
                );
              }}
              onSettingsChange={setModelSettings}
              onClose={closePicker}
            />
          ) : null}

          {picker === "project" ? (
            <div className="flex min-h-0 flex-col border-t border-stroke">
              <label className="flex shrink-0 items-center gap-2 px-4 py-2 text-content/45">
                <Search className="size-3.5 shrink-0" />
                <input
                  ref={queryRef}
                  value={query}
                  onChange={(event) => {
                    setQuery(event.target.value);
                    setHighlight(0);
                  }}
                  onKeyDown={onQueryKeyDown}
                  onBlur={(event) => {
                    if (
                      !frameRef.current?.contains(
                        event.relatedTarget as Node | null,
                      )
                    ) {
                      closePicker();
                    }
                  }}
                  placeholder="Find a project"
                  aria-label="Find a project"
                  spellCheck={false}
                  autoComplete="off"
                  className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35"
                />
              </label>
              <div
                ref={listRef}
                role="listbox"
                aria-label="Projects"
                className="min-h-0 max-h-64 overflow-y-auto overscroll-none px-2 pb-2"
              >
                {optionCount === 0 ? (
                  <p className="px-2 py-2 text-[12px] text-content/45">
                    No matches
                  </p>
                ) : (
                  projectOptions.map((path, index) => (
                    <button
                      key={path}
                      type="button"
                      role="option"
                      aria-selected={index === highlight}
                      tabIndex={-1}
                      onMouseEnter={() => setHighlight(index)}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => chooseAt(index)}
                      className={optionClass(index)}
                    >
                      <QuickProjectIcon
                        projectPath={path}
                        appearance={projectAppearance}
                        className="size-3 shrink-0"
                      />
                      <span className="truncate">{projectName(path)}</span>
                      <span className="ml-auto truncate pl-3 text-[11px] text-content/40">
                        {prettyParent(path)}
                      </span>
                    </button>
                  ))
                )}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function Kbd({ children }: { children: string }) {
  return (
    <kbd className="rounded border border-content/12 px-1 font-sans text-[10px] text-content/55">
      {children}
    </kbd>
  );
}
