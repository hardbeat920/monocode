import {
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { HARNESS_TITLE, type Block } from "../../sessions/model/session";
import type {
  OrchestrationChoice,
  ProposedTask,
} from "../model/orchestrationPlan";
import { orchestrator } from "../model/orchestration";
import { resizeComposer } from "../../sessions/model/composerResize";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import {
  findModel,
  mergeModelSettings,
  modelEffortLabel,
  modelEffortSetting,
} from "../../sessions/model/models";
import { LAYER } from "../../../shared/lib/layers";
import { OrchestrationActions } from "./OrchestrationActions";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import { Popover } from "../../../shared/ui/Popover";
import {
  Check,
  ChevronDown,
  ChevronRight,
  CircleDashed,
  CircleHelp,
  MessageMultiple,
  Play,
  Search,
} from "../../../shared/ui/icons";

function AssignmentModel({
  task,
  choices,
  onChange,
}: {
  task: ProposedTask;
  choices: OrchestrationChoice[];
  onChange(
    choice: OrchestrationChoice,
    modelSettings: Record<string, string>,
  ): void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [effortActive, setEffortActive] = useState(0);
  const [inEffort, setInEffort] = useState(false);
  const [activeRow, setActiveRow] = useState<HTMLButtonElement | null>(null);
  const anchor = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const listOverscroll = useLockOverscroll<HTMLDivElement>();
  const selected = choices.find(
    (choice) => choice.harness === task.harness && choice.model === task.model,
  );
  const matches = choices.filter((choice) =>
    `${choice.name} ${choice.model} ${HARNESS_TITLE[choice.harness]}`
      .toLowerCase()
      .includes(query.trim().toLowerCase()),
  );
  const effortForChoice = (choice: OrchestrationChoice) => {
    const model = findModel(choice.model);
    return model?.harness === choice.harness
      ? modelEffortSetting(model)
      : undefined;
  };
  const activeChoice = matches[active];
  const activeModel = activeChoice ? findModel(activeChoice.model) : undefined;
  const effort = activeChoice ? effortForChoice(activeChoice) : undefined;
  const selectedEffortValue = effort
    ? activeChoice === selected
      ? (task.modelSettings?.[effort.id] ?? effort.value)
      : effort.value
    : undefined;
  const selectedModel = selected ? findModel(selected.model) : undefined;
  const selectedEffortLabel =
    selectedModel && selectedModel.harness === selected?.harness
      ? modelEffortLabel(selectedModel, task.modelSettings)
      : undefined;
  // Two things fought this field for focus. The composer takes focus back
  // unless a picker surface is in the DOM, which `data-model-picker` below
  // now declares; and the popover measures itself with `visibility: hidden`
  // on its first pass, where nothing can be focused. Placement flushes in a
  // layout effect ahead of this one, so focus now for the pass that is
  // already on screen and again next frame, once placement has landed.
  // Re-focusing a focused field is a no-op.
  useEffect(() => {
    if (!open) return;
    search.current?.focus({ preventScroll: true });
    const frame = requestAnimationFrame(() =>
      search.current?.focus({ preventScroll: true }),
    );
    return () => cancelAnimationFrame(frame);
  }, [open]);
  useEffect(() => {
    activeRow?.scrollIntoView({ block: "nearest" });
  }, [activeRow]);
  useEffect(() => {
    if (!effort) {
      setEffortActive(0);
      return;
    }
    const index = effort.options.findIndex(
      (option) => option.value === selectedEffortValue,
    );
    setEffortActive(index >= 0 ? index : 0);
  }, [effort, selectedEffortValue]);
  const pick = (
    choice: OrchestrationChoice,
    modelSettings: Record<string, string>,
  ) => {
    onChange(choice, modelSettings);
    setOpen(false);
    setInEffort(false);
    anchor.current?.focus();
  };
  const settingsFor = (choice: OrchestrationChoice, effortValue?: string) => {
    const model = findModel(choice.model);
    if (!model || model.harness !== choice.harness) return {};
    const setting = modelEffortSetting(model);
    return mergeModelSettings(model, {
      ...(choice === selected ? task.modelSettings : undefined),
      ...(setting && effortValue ? { [setting.id]: effortValue } : {}),
    });
  };
  const openEffortOrPick = (choice: OrchestrationChoice) => {
    const model = findModel(choice.model);
    if (
      model?.harness === choice.harness &&
      modelEffortSetting(model)?.options.length
    ) {
      setInEffort(true);
      return;
    }
    pick(choice, settingsFor(choice));
  };
  // Arrows walk the list from the search field, the way the app's other
  // pickers work. The keys stop here so the transcript underneath does not
  // scroll along with the highlight.
  const onSearchKey = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      if (inEffort && effort?.options.length) {
        const direction = event.key === "ArrowDown" ? 1 : -1;
        setEffortActive(
          (index) =>
            (index + direction + effort.options.length) % effort.options.length,
        );
        return;
      }
      if (!matches.length) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((index) =>
        Math.min(matches.length - 1, Math.max(0, index + step)),
      );
      return;
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      event.stopPropagation();
      if (effort?.options.length) setInEffort(true);
      return;
    }
    if (event.key === "ArrowLeft" && inEffort) {
      event.preventDefault();
      event.stopPropagation();
      setInEffort(false);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      if (inEffort) {
        const option = effort?.options[effortActive];
        if (activeChoice && option)
          pick(activeChoice, settingsFor(activeChoice, option.value));
        return;
      }
      if (activeChoice) openEffortOrPick(activeChoice);
    }
  };
  return (
    <div className="relative min-w-0">
      <button
        type="button"
        ref={anchor}
        aria-label={`Model for ${task.title}`}
        aria-expanded={open}
        onClick={() => {
          setQuery("");
          setActive(selected ? choices.indexOf(selected) : 0);
          setInEffort(false);
          setOpen(!open);
        }}
        className="flex h-7 max-w-full items-center gap-1.5 rounded-md bg-content/5 px-2 text-[11px] text-content/60 hover:bg-content/10 hover:text-content"
      >
        <HarnessIcon harness={task.harness} className="size-3.5 shrink-0" />
        <span className="truncate">
          {selected?.name ?? task.model}
          {selectedEffortLabel ? ` · ${selectedEffortLabel}` : ""} ·{" "}
          {HARNESS_TITLE[task.harness]}
        </span>
        <ChevronDown className="size-3 shrink-0" />
      </button>
      {open && (
        <Popover
          anchor={anchor}
          side="bottom"
          align="start"
          width={260}
          maxHeight={320}
          ignore="[data-assignment-model-target]"
          onDismiss={() => {
            setOpen(false);
            setInEffort(false);
          }}
          data-model-picker
          data-assignment-model-target
          className="flex flex-col overflow-hidden"
        >
          <label className="flex shrink-0 items-center gap-2 border-b border-stroke px-3 py-2.5 text-content/50">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              ref={search}
              type="text"
              aria-label="Search assignment models"
              placeholder="Search models or harnesses…"
              spellCheck={false}
              autoComplete="off"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActive(0);
                setInEffort(false);
              }}
              onKeyDown={onSearchKey}
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/40"
            />
          </label>
          <div
            ref={listOverscroll}
            role="listbox"
            aria-label="Assignment models"
            className="min-h-0 flex-1 overflow-y-auto overscroll-none p-1"
          >
            {matches.map((choice, index) => (
              <button
                key={`${choice.harness}:${choice.model}`}
                ref={index === active ? setActiveRow : undefined}
                type="button"
                role="option"
                aria-selected={choice === selected}
                aria-haspopup={
                  effortForChoice(choice)?.options.length ? "menu" : undefined
                }
                aria-expanded={index === active && inEffort}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => {
                  setActive(index);
                  const model = findModel(choice.model);
                  setInEffort(
                    model?.harness === choice.harness &&
                      !!modelEffortSetting(model)?.options.length,
                  );
                }}
                onClick={() => {
                  setActive(index);
                  openEffortOrPick(choice);
                }}
                className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left ${
                  index === active ? "bg-selection" : ""
                }`}
              >
                <HarnessIcon
                  harness={choice.harness}
                  className="size-4 shrink-0"
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] leading-tight text-content">
                    {choice.name}
                  </span>
                  <span className="mt-0.5 block truncate text-[11px] leading-tight text-content/45">
                    {HARNESS_TITLE[choice.harness]}
                  </span>
                </span>
                {choice === selected && (
                  <Check
                    className="size-3.5 shrink-0 text-content/55"
                    strokeWidth={2}
                  />
                )}
                {effortForChoice(choice)?.options.length ? (
                  <ChevronRight
                    className="size-3.5 shrink-0 text-content/40"
                    strokeWidth={1.75}
                  />
                ) : null}
              </button>
            ))}
            {!matches.length && (
              <p className="px-2 py-3 text-[12px] text-content/45">
                No matching models
              </p>
            )}
          </div>
        </Popover>
      )}
      {open &&
      inEffort &&
      activeRow &&
      activeChoice &&
      activeModel?.harness === activeChoice.harness &&
      effort ? (
        <Popover
          anchor={activeRow}
          side="right"
          gap={-4}
          width={200}
          layer={LAYER.submenu}
          role="menu"
          aria-label={`${activeChoice.name} effort`}
          ignore="[data-assignment-model-target]"
          onMouseEnter={() => setInEffort(true)}
          data-assignment-model-target
          className="p-1 font-sans"
        >
          {effort.options.map((option, index) => {
            const highlighted = index === effortActive;
            const selectedOption = option.value === selectedEffortValue;
            return (
              <button
                key={option.value}
                type="button"
                role="menuitemradio"
                aria-checked={selectedOption}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setEffortActive(index)}
                onClick={() =>
                  pick(activeChoice, settingsFor(activeChoice, option.value))
                }
                className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] text-content ${
                  highlighted ? "bg-selection" : "hover:bg-content/5"
                }`}
              >
                <span className="min-w-0 flex-1 truncate">{option.label}</span>
                {selectedOption ? (
                  <Check
                    className="size-3.5 shrink-0 text-content/50"
                    strokeWidth={2}
                  />
                ) : null}
              </button>
            );
          })}
        </Popover>
      ) : null}
    </div>
  );
}

function WorkerHelp() {
  const [hovered, setHovered] = useState(false);
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        type="button"
        ref={anchor}
        aria-label="What parallel workers means"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onFocus={() => setHovered(true)}
        onBlur={() => setHovered(false)}
        className="grid size-4 shrink-0 place-items-center rounded-full text-content/35 hover:text-content/70"
      >
        <CircleHelp className="size-3.5" strokeWidth={1.75} />
      </button>
      {(hovered || open) && (
        <Popover
          anchor={anchor}
          side="top"
          align="start"
          width={250}
          onDismiss={() => setOpen(false)}
          className={`px-2.5 py-2 ${open ? "" : "pointer-events-none"}`}
        >
          <div className="text-[12px] leading-4 text-content">
            How many workers run at once
          </div>
          <div className="mt-1 text-[11px] leading-4 text-content/50">
            The rest of the tasks wait their turn, and a task that depends on
            another waits for it either way. Every worker edits this same
            project folder, so a lower number means fewer changes landing in it
            at the same time.
          </div>
        </Popover>
      )}
    </>
  );
}

function InstructionsField({
  label,
  value,
  className,
  onChange,
}: {
  label: string;
  value: string;
  className: string;
  onChange(value: string): void;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLTextAreaElement>();
  // The same growth the composer uses: fit the text, stop at `max-h-40` and
  // scroll from there. The value is controlled, so one layout effect covers
  // typing and edits that arrive from the lead alike.
  useLayoutEffect(() => {
    if (ref.current) resizeComposer(ref.current);
  }, [value]);
  return (
    <textarea
      ref={(el) => {
        ref.current = el;
        lockOverscroll(el);
      }}
      rows={1}
      aria-label={label}
      className={`${className} max-h-40 resize-none overscroll-none`}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

export function OrchestrationPreview({
  block,
  busy,
}: {
  block: Block;
  busy?: boolean;
}) {
  const actions = useContext(OrchestrationActions);
  const runs = useSyncExternalStore(
    orchestrator.subscribe,
    orchestrator.snapshot,
    orchestrator.snapshot,
  );
  const proposal = block.orchestration!;
  const run = runs.find(
    (entry) =>
      entry.leadId === proposal.leadId && entry.proposalId === block.id,
  );
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();
  const [expanded, setExpanded] = useState<string[]>([]);
  const [showAll, setShowAll] = useState(false);
  useEffect(() => {
    if (actions)
      void orchestrator
        .hydrate(proposal.leadId)
        .catch((reason: unknown) => setError(String(reason)));
  }, [actions, proposal.leadId]);
  const editable =
    proposal.status === "ready" && !run && !pending && !busy && !!actions;
  const planning = proposal.status === "planning";
  const starting = pending || proposal.status === "starting";
  const visible = showAll ? proposal.tasks : proposal.tasks.slice(0, 3);
  const perform = async (fn: () => Promise<void>) => {
    setPending(true);
    setError(undefined);
    try {
      await fn();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setPending(false);
    }
  };
  const change = (id: string, patch: Partial<ProposedTask>) => {
    setError(undefined);
    actions?.update(proposal.leadId, block.id, {
      ...proposal,
      tasks: proposal.tasks.map((task) =>
        task.id === id ? { ...task, ...patch } : task,
      ),
    });
  };
  const secondary =
    "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[11px] text-content/50 hover:bg-content/8 hover:text-content disabled:opacity-35";
  const field =
    "w-full rounded-md border border-content/12 bg-background-base/40 px-2 py-1.5 text-[12px] leading-5 text-content outline-none placeholder:text-content/35 focus:border-content/30";
  const fieldLabel = "mb-1 block text-[11px] leading-tight text-content/45";
  return (
    <div
      className="mb-2 overflow-hidden rounded-xl border border-content/10 bg-content/3 font-sans"
      aria-label="Orchestration proposal"
      data-orchestration-review
    >
      <div className="flex min-w-0 flex-wrap items-center gap-2.5 px-3 py-2.5">
        <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-content/8 text-content/55">
          {planning || proposal.status === "starting" ? (
            <CircleDashed className="size-4 animate-spin" />
          ) : (
            <MessageMultiple className="size-4" strokeWidth={1.75} />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-medium leading-tight text-content/90">
            {planning ? "Planning assignments…" : proposal.title}
          </div>
          <div className="mt-1 flex items-center gap-1.5 text-[11px] leading-tight text-content/45">
            <HarnessIcon
              harness={proposal.author.harness}
              className="size-3 shrink-0"
            />
            <span
              className="truncate"
              title={HARNESS_TITLE[proposal.author.harness]}
            >
              Lead · {proposal.author.name}
            </span>
            {!!proposal.tasks.length && (
              <span className="shrink-0">
                · {proposal.tasks.length}{" "}
                {proposal.tasks.length === 1 ? "task" : "tasks"}
              </span>
            )}
          </div>
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-0.5">
          {!run && proposal.status === "invalid" && (
            <button
              className={secondary}
              disabled={busy || !actions}
              onClick={() => actions?.retry(proposal.leadId, block.id)}
            >
              Try again
            </button>
          )}
          {!run && ["ready", "starting"].includes(proposal.status) && (
            <button
              className="flex h-7 items-center gap-1.5 rounded-md bg-content px-2.5 text-[11px] font-medium text-background-base hover:bg-content/80 disabled:opacity-40"
              disabled={!editable}
              onClick={() =>
                void perform(() => actions!.confirm(proposal.leadId, block.id))
              }
            >
              <Play className="size-3" strokeWidth={1.75} />
              {starting ? "Starting…" : "Confirm & start"}
            </button>
          )}
          {run && (
            <button
              className={secondary}
              onClick={() =>
                actions?.openAgents?.(
                  run.tasks.map((task) => ({
                    sessionId: task.sessionId,
                    leadId: run.leadId,
                    title: task.title,
                    harness: task.harness,
                  })),
                )
              }
            >
              View agents
            </button>
          )}
        </div>
      </div>
      {planning && (
        <p className="px-3 pb-2.5 text-[12px] leading-5 text-content/50">
          {proposal.settings.choices.length
            ? "Your lead is choosing tasks and worker models. Review the assignments here before starting."
            : "Checking available harnesses and models…"}
        </p>
      )}
      {!!proposal.tasks.length && (
        <ul className="border-t border-stroke py-1">
          {visible.map((task) => {
            const index = proposal.tasks.indexOf(task);
            const open = expanded.includes(task.id);
            const taskChoice = proposal.settings.choices.find(
              (choice) =>
                choice.harness === task.harness && choice.model === task.model,
            );
            const taskModel = findModel(task.model);
            const taskEffort =
              taskModel?.harness === task.harness
                ? modelEffortLabel(taskModel, task.modelSettings)
                : undefined;
            return (
              <li key={task.id}>
                <div className="flex min-h-9 min-w-0 flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1 hover:bg-content/5">
                  <button
                    type="button"
                    aria-label={`Details for ${task.title}`}
                    aria-expanded={open}
                    onClick={() =>
                      setExpanded((prev) =>
                        open
                          ? prev.filter((id) => id !== task.id)
                          : [...prev, task.id],
                      )
                    }
                    className="flex min-w-0 flex-1 items-center gap-2 py-1 text-left text-content/65 hover:text-content"
                  >
                    {open ? (
                      <ChevronDown className="size-3.5 shrink-0" />
                    ) : (
                      <ChevronRight className="size-3.5 shrink-0" />
                    )}
                    <span className="truncate text-[12px]" title={task.title}>
                      {task.title}
                    </span>
                  </button>
                  <div className="max-w-[60%] min-w-0">
                    {editable ? (
                      <AssignmentModel
                        task={task}
                        choices={proposal.settings.choices}
                        onChange={(choice, modelSettings) =>
                          change(task.id, {
                            harness: choice.harness,
                            model: choice.model,
                            modelSettings,
                          })
                        }
                      />
                    ) : (
                      <span
                        className="flex min-w-0 items-center gap-1.5 text-[11px] text-content/50"
                        title={HARNESS_TITLE[task.harness]}
                      >
                        <HarnessIcon
                          harness={task.harness}
                          className="size-3 shrink-0"
                        />
                        <span className="truncate">
                          {taskChoice?.name ?? task.model}
                          {taskEffort ? ` · ${taskEffort}` : ""}
                        </span>
                      </span>
                    )}
                  </div>
                </div>
                {open && (
                  <div className="space-y-2.5 px-3 pb-3 pl-8 text-[11px] leading-4 text-content/45">
                    {editable ? (
                      <>
                        <label className="block">
                          <span className={fieldLabel}>Task</span>
                          <input
                            aria-label={`Title for task ${index + 1}`}
                            className={field}
                            value={task.title}
                            onChange={(event) =>
                              change(task.id, { title: event.target.value })
                            }
                          />
                        </label>
                        <label className="block">
                          <span className={fieldLabel}>Instructions</span>
                          <InstructionsField
                            label={`Instructions for task ${index + 1}`}
                            value={task.prompt}
                            className={field}
                            onChange={(prompt) => change(task.id, { prompt })}
                          />
                        </label>
                      </>
                    ) : (
                      <p className="whitespace-pre-wrap text-[12px] leading-5 text-content/60">
                        {task.prompt}
                      </p>
                    )}
                    {!!task.dependsOn.length && (
                      <p>
                        After ·{" "}
                        {task.dependsOn
                          .map(
                            (id) =>
                              proposal.tasks.find((entry) => entry.id === id)
                                ?.title ?? id,
                          )
                          .join(", ")}
                      </p>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {proposal.tasks.length > 3 && (
        <button
          type="button"
          aria-expanded={showAll}
          onClick={() => setShowAll(!showAll)}
          className="flex h-8 w-full items-center gap-1.5 border-t border-stroke px-3 text-left text-[11px] text-content/45 hover:bg-content/5 hover:text-content/70"
        >
          {showAll ? (
            <ChevronDown className="size-3.5" />
          ) : (
            <ChevronRight className="size-3.5" />
          )}
          {showAll
            ? "Show fewer tasks"
            : `Show ${proposal.tasks.length - 3} more ${proposal.tasks.length === 4 ? "task" : "tasks"}`}
        </button>
      )}
      {(error || proposal.error) && (
        <p role="alert" className="px-3 py-2 text-[12px] text-red-400">
          {error ?? proposal.error}
        </p>
      )}
      {!planning && (
        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-stroke px-3 py-2 text-[11px] text-content/45">
          <div className="flex items-center gap-1.5">
            {editable ? (
              <>
                <span>Parallel workers</span>
                <div
                  role="radiogroup"
                  aria-label="Parallel workers"
                  className="flex items-center gap-0.5 rounded-md bg-content/5 p-0.5"
                >
                  {[1, 2, 3, 4].map((number) => (
                    <button
                      key={number}
                      type="button"
                      role="radio"
                      aria-checked={proposal.settings.maxWorkers === number}
                      onClick={() =>
                        actions?.update(proposal.leadId, block.id, {
                          ...proposal,
                          settings: {
                            ...proposal.settings,
                            maxWorkers: number,
                          },
                        })
                      }
                      className={`grid size-5 place-items-center rounded-[5px] text-[11px] leading-none tabular-nums ${
                        proposal.settings.maxWorkers === number
                          ? "bg-selection-hover font-medium text-content"
                          : "text-content/45 hover:bg-content/8 hover:text-content"
                      }`}
                    >
                      {number}
                    </button>
                  ))}
                </div>
              </>
            ) : (
              <span>{proposal.settings.maxWorkers} parallel</span>
            )}
            <WorkerHelp />
          </div>
          <span>
            {run?.status ??
              (proposal.status === "approved"
                ? "Approved"
                : proposal.status === "ready"
                  ? "Awaiting confirmation"
                  : "")}{" "}
            · Shared project folder
          </span>
        </div>
      )}
    </div>
  );
}
