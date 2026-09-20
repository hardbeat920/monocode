import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type FormEvent,
  type ReactNode,
} from "react";
import { AccessPicker } from "../../sessions/ui/AccessPicker";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import {
  AlertCircle,
  CheckCircle,
  ChevronDown,
  ChevronRight,
  Clock,
  File,
  Gauge,
  GitPullRequest,
  Inbox,
  LoaderCircle,
  Lock,
  MoreHorizontal,
  Play,
  Plus,
  Search,
  StickyNote,
  Terminal,
  Trash2,
  X,
  Zap,
} from "../../../shared/ui/icons";
import { InboxProviderMark } from "../../inbox/ui/InboxProviderMark";
import { ModelControlPills, ModelPicker } from "../../sessions/ui/ModelPicker";
import { Popover } from "../../../shared/ui/Popover";
import { ProjectLogoIcon } from "../../projects/ui/ProjectLogoIcon";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import { SearchableProjectPicker } from "../../projects/ui/SearchableProjectPicker";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { SkillPromptField } from "../../skills/ui/SkillPromptField";
import { OverlayNav } from "../../../app/shell/TitleBar";
import { WindowControls } from "../../../app/shell/WindowControls";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { useTabGroupLogos } from "../../projects/hooks/useTabGroupLogos";
import {
  AUTOMATION_WEEKDAYS,
  applyTriggers,
  automationScheduleLabel,
  automationTriggers,
  createAutomationTrigger,
  createManualAutomationRun,
  deleteAutomation,
  draftFromAutomation,
  draftFromTemplate,
  formatAutomationRunAt,
  formatAutomationRunDuration,
  gmtOffsetLabel,
  listAutomationRuns,
  listAutomations,
  newAutomationDraft,
  nextAutomationRunAt,
  nextRunPreview,
  saveAutomation,
  setAutomationEnabled,
  subscribeAutomations,
  type Automation,
  type AutomationDraft,
  type AutomationRun,
  type AutomationTrigger,
  type AutomationTriggerKind,
} from "../model/automations";
import {
  AUTOMATION_TEMPLATE_CATEGORIES,
  templatesForCategory,
  type AutomationTemplate,
  type AutomationTemplateCategoryId,
  type AutomationTemplateIcon,
} from "../model/automationTemplates";
import {
  AZUREDEVOPS_CHANGE_EVENT,
  azureDevOpsConnected,
} from "../../inbox/model/azureDevOps";
import { gitBranches } from "../../../platform/tauri/fs";
import { formatRelativeTime, githubStatus } from "../../inbox/model/githubTasks";
import { GITLAB_CHANGE_EVENT, gitlabConnected } from "../../inbox/model/gitlab";
import { LAYER } from "../../../shared/lib/layers";
import { LINEAR_CHANGE_EVENT, linearConnected } from "../../inbox/model/linear";
import { defaultSessionChoice, modelsFor, resolveModel } from "../../sessions/model/models";
import { projectKey, projectName } from "../../../shared/lib/paths";
import { IS_MAC } from "../../../platform/tauri/platform";
import { looksLikeProject, type RecentProject } from "../../projects/model/recents";
import {
  loadSessionFolders,
  subscribeSessionFolders,
} from "../../sessions/model/sessionFolders";
import { loadModelControls, subscribeModelControls } from "../../settings/model/settings";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLabel,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";

type Props = {
  besideRail?: boolean;
  compactRail?: boolean;
  cwd?: string;
  recents: RecentProject[];
  onClose: () => void;
  onToggleSidebar?: () => void;
  onLaunch: (
    automation: Automation,
    run: AutomationRun,
  ) => void | Promise<void>;
  onOpenSession: (sessionId: string) => void | Promise<void>;
};

const ACTION =
  "inline-flex items-center gap-1.5 rounded-md px-3 text-[12px] disabled:cursor-default disabled:opacity-40";
const ACTION_FILLED = `${ACTION} h-6.5 bg-content font-medium text-background-base hover:bg-content/80`;
const ACTION_OUTLINE = `${ACTION} h-7 border border-content/15 text-content/80 hover:border-content/30 hover:bg-content/10 hover:text-content`;

let rememberedAutomationId: string | null = null;

export function AutomationsView({
  besideRail = false,
  compactRail = false,
  cwd,
  recents,
  onClose,
  onToggleSidebar,
  onLaunch,
  onOpenSession,
}: Props) {
  return (
    <div
      role="region"
      aria-label="Automations"
      data-app-automations
      className="flex min-h-0 min-w-0 flex-1 flex-col text-content"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center border-b border-stroke"
        data-tauri-drag-region="deep"
      >
        {IS_MAC && compactRail ? <div className="w-4 shrink-0" /> : null}
        {IS_MAC && !besideRail ? <div className="w-[78px] shrink-0" /> : null}
        {besideRail ? null : (
          <OverlayNav onBack={onClose} onToggleSidebar={onToggleSidebar} />
        )}
        <div className="flex min-w-0 flex-1 items-center gap-2 px-3 text-[13px]">
          <Zap
            className="size-3.5 shrink-0 text-content/45"
            strokeWidth={1.75}
          />
          <span className="min-w-0 truncate text-content">Automations</span>
        </div>
        {IS_MAC ? null : <WindowControls />}
      </div>
      <AutomationsContent
        cwd={cwd}
        recents={recents}
        onLaunch={onLaunch}
        onOpenSession={onOpenSession}
      />
    </div>
  );
}

function AutomationsContent({
  cwd,
  recents,
  onLaunch,
  onOpenSession,
}: Pick<Props, "cwd" | "recents" | "onLaunch" | "onOpenSession">) {
  const [automations, setAutomations] = useState<Automation[]>([]);
  const [runs, setRuns] = useState<AutomationRun[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(
    rememberedAutomationId,
  );
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<AutomationDraft | null>(null);
  const [pickerOpen, setPickerOpen] = useState(true);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState<string | null>(null);
  const logos = useTabGroupLogos();
  const [groupLabels] = useState(loadTabGroupLabels);
  const [groupMascots] = useState(loadTabGroupMascots);
  const [groupColors] = useState(loadTabGroupColors);
  const [groupCustomColors] = useState(loadTabGroupCustomColors);

  const refresh = useCallback(async () => {
    try {
      const next = await listAutomations();
      setAutomations(next);
      setSelectedId((current) => {
        const preferred = current ?? rememberedAutomationId;
        return preferred && next.some((entry) => entry.id === preferred)
          ? preferred
          : (next[0]?.id ?? null);
      });
      setError(null);
    } catch (reason: unknown) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    return subscribeAutomations(() => void refresh());
  }, [refresh]);

  useEffect(() => {
    rememberedAutomationId = selectedId;
    if (!selectedId) {
      setRuns([]);
      return;
    }
    let cancelled = false;
    void listAutomationRuns(selectedId)
      .then((next) => {
        if (!cancelled) setRuns(next);
      })
      .catch(() => {
        if (!cancelled) setRuns([]);
      });
    return () => {
      cancelled = true;
    };
  }, [automations, selectedId]);

  const visible = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return automations.filter((automation) => {
      if (!needle) return true;
      const project = resolveTabGroupLabel(
        projectKey(automation.cwd),
        groupLabels,
        projectName(automation.cwd),
      );
      return `${automation.name}\n${automation.prompt}\n${project}\n${automation.cwd}`
        .toLocaleLowerCase()
        .includes(needle);
    });
  }, [automations, groupLabels, query]);
  const selected = automations.find((entry) => entry.id === selectedId) ?? null;

  const defaultDraftTarget = () => {
    const preferred = defaultSessionChoice();
    const harness = selected?.harness ?? preferred.harness;
    const model =
      (selected?.harness === harness ? selected.model : undefined) ??
      modelsFor(harness)[0]?.id ??
      preferred.model;
    const project =
      cwd && looksLikeProject(cwd) ? cwd : (recents[0]?.path ?? "~");
    return { project, harness, model };
  };

  const beginCreate = () => {
    setPickerOpen(true);
    setDraft(null);
  };

  const beginBlank = () => {
    const { project, harness, model } = defaultDraftTarget();
    setPickerOpen(false);
    setDraft(newAutomationDraft(project, harness, model));
  };

  const beginFromTemplate = (template: AutomationTemplate) => {
    const { project, harness, model } = defaultDraftTarget();
    setPickerOpen(false);
    setDraft(draftFromTemplate(project, harness, model, template));
  };

  const onSave = async (event: FormEvent, nextDraft: AutomationDraft) => {
    event.preventDefault();
    if (saving) return;
    setSaving(true);
    try {
      const saved = await saveAutomation(nextDraft);
      setPickerOpen(false);
      setDraft(null);
      setSelectedId(saved.id);
      await refresh();
    } catch (reason: unknown) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSaving(false);
    }
  };

  const onRun = async (automation: Automation) => {
    if (running) return;
    setRunning(automation.id);
    try {
      const run = await createManualAutomationRun(automation.id);
      await onLaunch(automation, run);
    } catch (reason: unknown) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRunning(null);
    }
  };

  const onDelete = async (automation: Automation) => {
    if (!window.confirm(`Delete “${automation.name}” and its run history?`))
      return;
    try {
      await deleteAutomation(automation.id);
      setSelectedId(null);
      await refresh();
    } catch (reason: unknown) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const onToggle = async (automation: Automation, enabled: boolean) => {
    try {
      await setAutomationEnabled(automation, enabled);
      await refresh();
    } catch (reason: unknown) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const editorDraft = pickerOpen
    ? null
    : (draft ?? (selected ? draftFromAutomation(selected) : null));
  const listLock = useLockOverscroll<HTMLDivElement>();

  return (
    <div className="flex min-h-0 min-w-0 flex-1 text-content">
      <aside className="flex w-[280px] shrink-0 flex-col border-r border-stroke">
        <div className="flex h-9 shrink-0 items-center gap-1 border-b border-stroke px-2">
          <label className="relative flex h-7 min-w-0 flex-1 items-center">
            <Search className="pointer-events-none absolute left-2 size-3 shrink-0 text-content/40" />
            <span className="sr-only">Filter automations</span>
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter automations"
              spellCheck={false}
              autoComplete="off"
              className="h-7 w-full rounded-md bg-transparent pl-7 pr-2 text-[12px] outline-none placeholder:text-content/40"
            />
          </label>
          <button
            type="button"
            title="New automation"
            aria-label="New automation"
            onClick={beginCreate}
            className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
          >
            <Plus className="size-3.5" strokeWidth={1.75} />
          </button>
        </div>
        <div
          ref={listLock}
          className="min-h-0 flex-1 overflow-y-auto overscroll-none p-1.5"
        >
          {loading ? (
            <div className="grid place-items-center py-12 text-content/35">
              <LoaderCircle className="size-4 animate-spin" />
            </div>
          ) : visible.length > 0 ? (
            <ul className="space-y-0.5">
              {visible.map((automation) => (
                <li key={automation.id}>
                  <AutomationCard
                    automation={automation}
                    active={
                      !pickerOpen &&
                      automation.id === selected?.id &&
                      (!draft || draft.id === automation.id)
                    }
                    logos={logos}
                    labels={groupLabels}
                    mascots={groupMascots}
                    colors={groupColors}
                    customColors={groupCustomColors}
                    onSelect={() => {
                      setPickerOpen(false);
                      setDraft(null);
                      setSelectedId(automation.id);
                    }}
                    onToggle={(enabled) => void onToggle(automation, enabled)}
                  />
                </li>
              ))}
            </ul>
          ) : (
            <p className="px-3 py-8 text-center text-[12px] text-content/45">
              {query.trim() ? "No matching automations" : "No automations yet"}
            </p>
          )}
        </div>
      </aside>

      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
        {error ? (
          <div className="m-4 flex shrink-0 items-start gap-2 rounded-lg border border-red-400/20 bg-red-400/8 px-3 py-2 text-[12px] text-red-300">
            <AlertCircle className="mt-0.5 size-3.5 shrink-0" />
            <span>{error}</span>
          </div>
        ) : null}
        {loading && !editorDraft ? (
          <div className="grid min-h-0 flex-1 place-items-center text-content/35">
            <LoaderCircle className="size-4 animate-spin" />
          </div>
        ) : editorDraft ? (
          <AutomationEditor
            key={editorDraft.id ?? "new"}
            draft={editorDraft}
            recents={recents}
            runs={editorDraft.id ? runs : []}
            saving={saving}
            running={editorDraft.id === running}
            dirty={draft != null}
            onChange={setDraft}
            onClose={() => {
              setDraft(null);
              if (!editorDraft.id) setPickerOpen(true);
            }}
            onSubmit={(event) => onSave(event, editorDraft)}
            onRun={
              selected && editorDraft.id
                ? () => void onRun(selected)
                : undefined
            }
            onDelete={
              selected && editorDraft.id
                ? () => void onDelete(selected)
                : undefined
            }
            onOpenSession={async (sessionId) => {
              try {
                await onOpenSession(sessionId);
              } catch (reason: unknown) {
                setError(
                  reason instanceof Error ? reason.message : String(reason),
                );
              }
            }}
          />
        ) : (
          <AutomationPicker onBlank={beginBlank} onPick={beginFromTemplate} />
        )}
      </main>
    </div>
  );
}

function AutomationCard({
  automation,
  active,
  logos,
  labels,
  mascots,
  colors,
  customColors,
  onSelect,
  onToggle,
}: {
  automation: Automation;
  active: boolean;
  logos: Record<string, string>;
  labels: Record<string, string>;
  mascots: Record<string, string>;
  colors: Record<string, number>;
  customColors: Record<string, string>;
  onSelect: () => void;
  onToggle: (enabled: boolean) => void;
}) {
  const seed = projectName(automation.cwd);
  const key = projectKey(automation.cwd);
  const project = resolveTabGroupLabel(key, labels, seed);
  const logoPath = resolveTabGroupLogo(key, logos);
  const mascotName = resolveTabGroupMascot(key, mascots);
  const mascotColor = resolveTabGroupColor(key, colors, customColors, seed);
  const lastRun = automation.lastRunAt
    ? formatRelativeTime(new Date(automation.lastRunAt).toISOString())
    : null;
  const model = resolveModel(automation.harness, automation.model);
  return (
    <div
      className={`group rounded-md border px-2.5 py-2 ${
        active
          ? "border-transparent bg-selection"
          : "border-transparent hover:bg-content/5"
      }`}
    >
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          aria-current={active ? "true" : undefined}
          onClick={onSelect}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-[10px] text-content/45"
        >
          <TriggerMark
            kind={
              automationTriggers(automation)[0]?.kind ?? automation.triggerKind
            }
            className="size-3"
          />
          <span className="min-w-0 truncate">{triggerLabel(automation)}</span>
        </button>
        <ToggleSwitch
          label={`${automation.enabled ? "Pause" : "Enable"} ${automation.name}`}
          on={automation.enabled}
          onChange={onToggle}
          compact
        />
      </div>
      <button
        type="button"
        onClick={onSelect}
        className="mt-1 w-full text-left"
      >
        <span className="block truncate text-[13px] font-semibold text-content">
          {automation.name}
        </span>
        <span className="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-content/45">
          {logoPath ? (
            <ProjectLogoIcon
              path={logoPath}
              className="size-3.5 shrink-0 rounded-sm"
              imageClassName="size-3.5"
            />
          ) : (
            <ProjectMascot
              project={seed}
              color={mascotColor}
              name={mascotName}
              className="size-3 shrink-0"
            />
          )}
          <span className="min-w-0 truncate">{project}</span>
          {lastRun ? (
            <>
              <span className="shrink-0">·</span>
              <span className="shrink-0">{lastRun}</span>
            </>
          ) : null}
          <span
            title={model.name}
            className="ml-auto flex min-w-0 max-w-28 items-center gap-1 text-content/50"
          >
            <HarnessIcon
              harness={automation.harness}
              className="size-3.5 shrink-0"
            />
            <span className="min-w-0 truncate">{model.name}</span>
          </span>
        </span>
      </button>
    </div>
  );
}

function AutomationPicker({
  onBlank,
  onPick,
}: {
  onBlank: () => void;
  onPick: (template: AutomationTemplate) => void;
}) {
  const [category, setCategory] =
    useState<AutomationTemplateCategoryId>("popular");
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const templates = templatesForCategory(category);

  return (
    <div
      ref={lockOverscroll}
      className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-none"
    >
      <div className="mx-auto w-full max-w-5xl px-8 pt-5 pb-10">
        <h1 className="text-[20px] font-semibold leading-tight text-content">
          New automation
        </h1>
        <p className="mt-1.5 text-[13px] text-content/50">
          Pick an example or start from scratch.
        </p>
        <div className="mt-4 flex flex-wrap gap-1.5">
          {AUTOMATION_TEMPLATE_CATEGORIES.map((option) => {
            const selected = option.id === category;
            return (
              <button
                key={option.id}
                type="button"
                onClick={() => setCategory(option.id)}
                className={`h-7 rounded-full px-3 text-[12px] font-medium ${
                  selected
                    ? "bg-content text-background-base"
                    : "text-content/55 hover:bg-content/8 hover:text-content"
                }`}
              >
                {option.label}
              </button>
            );
          })}
        </div>
        <div className="mt-4 grid grid-cols-1 gap-3 min-[780px]:grid-cols-2">
          <button
            type="button"
            onClick={onBlank}
            className="flex min-h-37 flex-col rounded-xl border border-dashed border-content/15 p-4 text-left hover:border-content/25 hover:bg-content/5"
          >
            <div className="flex gap-3">
              <span className="grid size-9 shrink-0 place-items-center rounded-full bg-content/8 text-content/70">
                <Plus className="size-4" strokeWidth={1.75} />
              </span>
              <span className="min-w-0">
                <span className="block text-[13px] font-medium text-content">
                  Start from scratch
                </span>
                <span className="mt-1 block text-[12px] leading-snug text-content/50">
                  Write your own instructions and choose a trigger.
                </span>
              </span>
            </div>
          </button>
          {templates.map((template) => {
            const Icon = TEMPLATE_ICONS[template.icon];
            return (
              <button
                key={template.id}
                type="button"
                onClick={() => onPick(template)}
                className="flex min-h-37 flex-col rounded-xl border border-content/10 p-4 text-left hover:border-content/16 hover:bg-content/5"
              >
                <div className="flex gap-3">
                  <span className="grid size-9 shrink-0 place-items-center rounded-full bg-content/8 text-content/70">
                    <Icon className="size-4" strokeWidth={1.75} />
                  </span>
                  <span className="min-w-0">
                    <span className="block text-[13px] font-medium text-content">
                      {template.name}
                    </span>
                    <span className="mt-1 block text-[12px] leading-snug text-content/50">
                      {template.description}
                    </span>
                  </span>
                </div>
                <span className="mt-auto flex min-w-0 items-center gap-1.5 pt-3 text-[11px] text-content/45">
                  <TriggerMark
                    kind={template.trigger.kind}
                    className="size-3"
                  />
                  <span className="min-w-0 truncate">
                    {template.triggerLabel}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const RUN_GRID =
  "grid grid-cols-[minmax(0,1.4fr)_9.5rem_6.75rem_3.5rem] items-center gap-4 px-4";

function RunRow({
  run,
  draft,
  onOpenSession,
}: {
  run: AutomationRun;
  draft: AutomationDraft;
  onOpenSession: (sessionId: string) => void | Promise<void>;
}) {
  const trigger = runTriggerMeta(run, draft);
  const sessionId = run.sessionId;
  return (
    <li>
      <button
        type="button"
        disabled={!sessionId}
        title={
          sessionId
            ? "Open session"
            : (run.error ?? "This run has no session yet")
        }
        onClick={() => {
          if (!sessionId) return;
          void onOpenSession(sessionId);
        }}
        className={`${RUN_GRID} h-11 w-full text-left text-[13px] disabled:opacity-100 ${
          sessionId ? "cursor-pointer hover:bg-content/5" : "cursor-default"
        }`}
      >
        <span className="flex min-w-0 items-center gap-2 text-content">
          <TriggerMark
            kind={trigger.kind}
            className="size-3.5 shrink-0 text-content/40"
          />
          <span className="min-w-0 truncate">{trigger.label}</span>
        </span>
        <span className="truncate text-content/70">
          {formatAutomationRunAt(run.scheduledFor || run.createdAt)}
        </span>
        <span>
          <RunStatusPill status={run.status} />
        </span>
        <span className="text-right tabular-nums text-content/55">
          {formatAutomationRunDuration(run)}
        </span>
      </button>
    </li>
  );
}

function RunStatusPill({ status }: { status: AutomationRun["status"] }) {
  const running = status === "running";
  return (
    <span
      className={`inline-flex h-5 max-w-full items-center gap-1 rounded-full px-2 text-[11px] font-medium ${runStatusTone(status)}`}
    >
      {running ? (
        <LoaderCircle className="size-2.5 shrink-0 animate-spin" />
      ) : null}
      {runStatusLabel(status)}
    </span>
  );
}

function runStatusLabel(status: AutomationRun["status"]): string {
  if (status === "succeeded") return "Succeeded";
  if (status === "failed") return "Failed";
  if (status === "skipped") return "Skipped";
  if (status === "cancelled") return "Cancelled";
  if (status === "running") return "Running";
  return "Pending";
}

function runStatusTone(status: AutomationRun["status"]): string {
  if (status === "succeeded") return "bg-emerald-500/12 text-emerald-400";
  if (status === "failed") return "bg-rose-500/12 text-rose-400";
  if (status === "skipped") return "bg-amber-500/12 text-amber-400";
  if (status === "cancelled") return "bg-content/8 text-content/50";
  return "bg-blue-500/12 text-blue-400";
}

function runTriggerMeta(
  run: AutomationRun,
  draft: AutomationDraft,
): { kind: AutomationTriggerKind; label: string } {
  if (run.trigger === "manual") return { kind: "time", label: "Test run" };
  if (run.trigger === "event") {
    const kind = run.eventKind ?? draft.triggerKind;
    const event = run.event ?? draft.triggerEvent;
    return {
      kind,
      label:
        findTriggerEvent(kind, event)?.label ??
        triggerName(kind),
    };
  }
  const times = draft.triggers.filter((trigger) => trigger.kind === "time");
  if (times[0]) {
    return {
      kind: "time",
      label: `Scheduled · ${automationScheduleLabel(times[0])}`,
    };
  }
  const first = draft.triggers[0];
  if (!first) return { kind: "time", label: "Scheduled" };
  const event =
    findTriggerEvent(first.kind, first.event)?.label ?? triggerName(first.kind);
  return { kind: first.kind, label: event };
}

function AutomationEditor({
  draft,
  recents,
  runs,
  saving,
  running,
  dirty,
  onChange,
  onClose,
  onSubmit,
  onRun,
  onDelete,
  onOpenSession,
}: {
  draft: AutomationDraft;
  recents: RecentProject[];
  runs: AutomationRun[];
  saving: boolean;
  running: boolean;
  dirty: boolean;
  onChange: (draft: AutomationDraft) => void;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
  onRun?: () => void;
  onDelete?: () => void;
  onOpenSession: (sessionId: string) => void | Promise<void>;
}) {
  const [tab, setTab] = useState<"settings" | "history">("settings");
  const settingsTabId = useId();
  const historyTabId = useId();
  const tabPanelId = useId();
  const [menuOpen, setMenuOpen] = useState(false);
  const [triggerOpen, setTriggerOpen] = useState(false);
  const [triggerQuery, setTriggerQuery] = useState("");
  const [triggerCategory, setTriggerCategory] =
    useState<AutomationTriggerKind | null>(null);
  const triggerButton = useRef<HTMLButtonElement>(null);
  const menuButton = useRef<HTMLButtonElement>(null);
  const triggerCategoryAnchors = useRef<
    Partial<Record<AutomationTriggerKind, HTMLButtonElement>>
  >({});
  const [providerConnected, setProviderConnected] = useState({
    github: false,
    linear: false,
    gitlab: false,
    azuredevops: false,
  });
  const controlsBeside =
    useSyncExternalStore(subscribeModelControls, loadModelControls) ===
    "beside";
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  useEffect(() => {
    let cancelled = false;
    const load = () => {
      void Promise.all([
        githubStatus()
          .then((status) => status.connected)
          .catch(() => false),
        linearConnected()
          .then((status) => status.connected)
          .catch(() => false),
        gitlabConnected()
          .then((status) => status.connected)
          .catch(() => false),
        azureDevOpsConnected()
          .then((status) => status.connected)
          .catch(() => false),
      ]).then(([github, linear, gitlab, azuredevops]) => {
        if (!cancelled) {
          setProviderConnected({ github, linear, gitlab, azuredevops });
        }
      });
    };
    load();
    window.addEventListener(LINEAR_CHANGE_EVENT, load);
    window.addEventListener(GITLAB_CHANGE_EVENT, load);
    window.addEventListener(AZUREDEVOPS_CHANGE_EVENT, load);
    return () => {
      cancelled = true;
      window.removeEventListener(LINEAR_CHANGE_EVENT, load);
      window.removeEventListener(GITLAB_CHANGE_EVENT, load);
      window.removeEventListener(AZUREDEVOPS_CHANGE_EVENT, load);
    };
  }, []);
  const triggerReady = (kind: AutomationTriggerKind) => {
    if (kind === "time") return true;
    return providerConnected[kind];
  };
  const valid =
    draft.name.trim().length > 0 &&
    draft.prompt.trim().length > 0 &&
    looksLikeProject(draft.cwd) &&
    draft.model.length > 0;
  const [sessionFolders, setSessionFolders] = useState(() =>
    loadSessionFolders(draft.cwd),
  );
  useEffect(() => {
    setSessionFolders(loadSessionFolders(draft.cwd));
    return subscribeSessionFolders(draft.cwd, () => {
      setSessionFolders(loadSessionFolders(draft.cwd));
    });
  }, [draft.cwd]);
  const folderOptions = useMemo(() => {
    const options = [
      { value: "", label: "None" },
      ...sessionFolders.map((folder) => ({
        value: folder.id,
        label: folder.name,
        keywords: folder.name,
      })),
    ];
    if (
      draft.sessionFolderId &&
      !sessionFolders.some((folder) => folder.id === draft.sessionFolderId)
    ) {
      options.push({
        value: draft.sessionFolderId,
        label: "Removed folder",
        keywords: draft.sessionFolderId,
      });
    }
    return options;
  }, [draft.sessionFolderId, sessionFolders]);
  const update = <K extends keyof AutomationDraft>(
    key: K,
    value: AutomationDraft[K],
  ) => onChange({ ...draft, [key]: value });
  const triggerCategories = useMemo(
    () =>
      TRIGGER_CATEGORIES.filter((option) =>
        option.label
          .toLocaleLowerCase()
          .includes(triggerQuery.toLocaleLowerCase()),
      ),
    [triggerQuery],
  );
  useEffect(() => {
    if (
      triggerCategory &&
      !triggerCategories.some((option) => option.value === triggerCategory)
    ) {
      setTriggerCategory(null);
    }
  }, [triggerCategories, triggerCategory]);
  const setTriggers = (triggers: AutomationTrigger[]) =>
    onChange(applyTriggers(draft, triggers));
  const selectTrigger = (kind: AutomationTriggerKind, event: TriggerEvent) => {
    if (draft.triggers.length >= 20 || !triggerReady(kind)) {
      setTriggerCategory(null);
      setTriggerOpen(false);
      return;
    }
    setTriggers([
      ...draft.triggers,
      createAutomationTrigger(kind, event.value),
    ]);
    setTriggerCategory(null);
    setTriggerOpen(false);
  };
  const updateTrigger = (next: AutomationTrigger) =>
    setTriggers(
      draft.triggers.map((trigger) =>
        trigger.id === next.id ? next : trigger,
      ),
    );

  const historyAvailable = Boolean(draft.id);
  const showingHistory = historyAvailable && tab === "history";

  return (
    <form className="flex min-h-0 min-w-0 flex-1 flex-col" onSubmit={onSubmit}>
      <div className="relative w-full shrink-0 border-b border-stroke">
        <div className="mx-auto flex w-full max-w-5xl flex-col gap-2.5 px-8 pt-5">
          <header className="flex flex-col gap-2.5">
            <div className="flex items-start gap-6">
              <input
                autoFocus
                aria-label="Automation name"
                value={draft.name}
                onChange={(event) => update("name", event.target.value)}
                placeholder="Untitled"
                className="min-w-0 flex-1 bg-transparent text-[20px] font-semibold leading-tight text-content outline-none placeholder:text-content/35"
              />
              <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
                {!draft.id || dirty ? (
                  <button
                    type="button"
                    onClick={onClose}
                    className={ACTION_OUTLINE}
                  >
                    {draft.id ? "Reset" : "Cancel"}
                  </button>
                ) : null}
                {onRun ? (
                  <button
                    type="button"
                    onClick={onRun}
                    disabled={running}
                    className={ACTION_OUTLINE}
                  >
                    {running ? (
                      <LoaderCircle className="size-3.5 animate-spin" />
                    ) : (
                      <Play className="size-3.5" />
                    )}
                    Run now
                  </button>
                ) : null}
                <button
                  type="submit"
                  disabled={!valid || saving || (!!draft.id && !dirty)}
                  className={ACTION_FILLED}
                >
                  {saving ? (
                    <LoaderCircle className="size-3.5 animate-spin" />
                  ) : null}
                  {draft.id ? "Save" : "Create"}
                </button>
              </div>
            </div>
            <div className="flex min-w-0 items-center gap-2 overflow-hidden whitespace-nowrap text-[12px] text-content/50">
              <ToggleSwitch
                label={draft.enabled ? "Pause automation" : "Enable automation"}
                on={draft.enabled}
                onChange={(enabled) => update("enabled", enabled)}
                compact
              />
              <span className="shrink-0">
                {draft.enabled ? "Active" : "Inactive"}
              </span>
              <span
                aria-hidden
                className="h-3 ml-2 w-px shrink-0 bg-content/15"
              />
              <SearchableProjectPicker
                cwd={draft.cwd}
                recents={recents}
                onSelectProject={(cwd) =>
                  onChange({ ...draft, cwd, sessionFolderId: "" })
                }
              />
              {onDelete ? (
                <>
                  <span
                    aria-hidden
                    className="h-3 w-px shrink-0 bg-content/15"
                  />
                  <button
                    ref={menuButton}
                    type="button"
                    title="Automation actions"
                    aria-label="Automation actions"
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    onClick={() => setMenuOpen((open) => !open)}
                    className={`grid size-6.5 shrink-0 place-items-center rounded-md ${
                      menuOpen
                        ? "bg-selection text-content"
                        : "text-content/50 hover:bg-content/5 hover:text-content"
                    }`}
                  >
                    <MoreHorizontal className="size-3.5" strokeWidth={1.75} />
                  </button>
                  {menuOpen ? (
                    <Popover
                      anchor={menuButton}
                      side="bottom"
                      align="start"
                      gap={4}
                      width={200}
                      constrainHeight={false}
                      role="menu"
                      aria-label="Automation actions"
                      onDismiss={() => setMenuOpen(false)}
                      className="p-1"
                    >
                      <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          onDelete();
                        }}
                        className="flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] text-red-300/90 hover:bg-red-500/15"
                      >
                        <Trash2 className="size-3.5" strokeWidth={1.75} />
                        Delete automation
                      </button>
                    </Popover>
                  ) : null}
                </>
              ) : null}
            </div>
          </header>
          {historyAvailable ? (
            <div
              role="tablist"
              aria-label="Automation view"
              className="flex h-9 items-stretch gap-4"
            >
              <PageTab
                id={settingsTabId}
                controls={tabPanelId}
                label="Settings"
                selected={!showingHistory}
                onSelect={() => setTab("settings")}
              />
              <PageTab
                id={historyTabId}
                controls={tabPanelId}
                label="Run history"
                selected={showingHistory}
                onSelect={() => setTab("history")}
              />
            </div>
          ) : (
            <div className="pb-5" />
          )}
        </div>
      </div>

      <div
        ref={lockOverscroll}
        id={historyAvailable ? tabPanelId : undefined}
        role={historyAvailable ? "tabpanel" : undefined}
        aria-labelledby={
          historyAvailable
            ? showingHistory
              ? historyTabId
              : settingsTabId
            : undefined
        }
        className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-none"
      >
        {!showingHistory ? (
          <div className="mx-auto w-full max-w-5xl space-y-8 px-8 py-5 pb-10">
            <section>
              <SectionTitle>Triggers</SectionTitle>
              <div className="mt-3 rounded-md border border-content/10">
                {draft.triggers.length > 0 ? (
                  <ul className="px-3 py-1.5">
                    {draft.triggers.map((trigger) => (
                      <li key={trigger.id}>
                        <TriggerRow
                          trigger={trigger}
                          cwd={draft.cwd}
                          onChange={updateTrigger}
                          onRemove={() =>
                            setTriggers(
                              draft.triggers.filter(
                                (entry) => entry.id !== trigger.id,
                              ),
                            )
                          }
                        />
                      </li>
                    ))}
                  </ul>
                ) : null}
                {draft.triggers.length > 0 ? (
                  <div className="border-t border-content/8" />
                ) : null}
                <button
                  ref={triggerButton}
                  type="button"
                  aria-haspopup="menu"
                  aria-expanded={triggerOpen}
                  disabled={draft.triggers.length >= 20}
                  onClick={() => {
                    setTriggerQuery("");
                    setTriggerCategory(null);
                    setTriggerOpen((open) => !open);
                  }}
                  className={`flex h-12 w-full items-center gap-2 px-3 text-left text-[13px] text-content/55 transition-colors duration-150 ease-out hover:bg-content/4 hover:text-content disabled:pointer-events-none disabled:opacity-40 ${
                    draft.triggers.length > 0 ? "rounded-b-md" : "rounded-md"
                  }`}
                >
                  <Plus className="size-3.5" />
                  Add Trigger
                </button>
              </div>
              {triggerOpen ? (
                <Popover
                  anchor={triggerButton}
                  side="bottom"
                  align="start"
                  gap={4}
                  width={250}
                  role="menu"
                  aria-label="Choose automation trigger"
                  layer={LAYER.popover}
                  ignore="[data-trigger-submenu]"
                  onDismiss={() => {
                    setTriggerCategory(null);
                    setTriggerOpen(false);
                  }}
                  className="overflow-hidden"
                >
                  <label className="flex h-11 items-center gap-2.5 border-b border-stroke px-3 text-content/45 focus-within:text-content/70">
                    <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
                    <span className="sr-only">Search triggers</span>
                    <input
                      autoFocus
                      value={triggerQuery}
                      onChange={(event) => setTriggerQuery(event.target.value)}
                      placeholder="Search triggers"
                      className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35"
                    />
                  </label>
                  <div className="p-1">
                    {triggerCategories.map((option) => {
                      const ready = triggerReady(option.value);
                      return (
                        <button
                          key={option.value}
                          ref={(node) => {
                            if (node)
                              triggerCategoryAnchors.current[option.value] =
                                node;
                            else
                              delete triggerCategoryAnchors.current[
                                option.value
                              ];
                          }}
                          type="button"
                          role="menuitem"
                          aria-haspopup={ready ? "menu" : undefined}
                          aria-expanded={triggerCategory === option.value}
                          aria-disabled={ready ? undefined : true}
                          title={
                            ready
                              ? undefined
                              : `Connect ${option.label} in Settings`
                          }
                          onMouseEnter={() =>
                            setTriggerCategory(ready ? option.value : null)
                          }
                          onFocus={() =>
                            setTriggerCategory(ready ? option.value : null)
                          }
                          onClick={() => {
                            if (!ready) return;
                            setTriggerCategory(option.value);
                          }}
                          className={`flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] ${
                            !ready
                              ? "cursor-default text-content/35"
                              : triggerCategory === option.value
                                ? "bg-selection text-content"
                                : "text-content/70 hover:bg-content/5 hover:text-content"
                          }`}
                        >
                          <TriggerMark
                            kind={option.value}
                            className="size-3.5 shrink-0"
                          />
                          <span className="min-w-0 flex-1 truncate">
                            {option.label}
                          </span>
                          {ready ? (
                            <ChevronRight className="size-3.5 text-content/45" />
                          ) : (
                            <span className="shrink-0 text-[11px] text-content/35">
                              Not connected
                            </span>
                          )}
                        </button>
                      );
                    })}
                    {triggerCategories.length === 0 ? (
                      <p className="px-2 py-5 text-center text-[12px] text-content/40">
                        No matching triggers
                      </p>
                    ) : null}
                  </div>
                </Popover>
              ) : null}
              {triggerOpen &&
              triggerCategory &&
              triggerCategories.some(
                (option) => option.value === triggerCategory,
              ) &&
              triggerReady(triggerCategory) ? (
                <Popover
                  anchor={
                    triggerCategoryAnchors.current[triggerCategory] ?? null
                  }
                  side="right"
                  align="start"
                  gap={4}
                  width={220}
                  maxHeight={360}
                  role="menu"
                  aria-label={`${triggerName(triggerCategory)} events`}
                  layer={LAYER.submenu}
                  data-trigger-submenu
                  onDismiss={(reason) => {
                    if (reason === "escape") setTriggerCategory(null);
                  }}
                  className="overflow-y-auto p-1"
                >
                  {TRIGGER_EVENTS[triggerCategory].map((event) => (
                    <button
                      key={event.value}
                      type="button"
                      role="menuitem"
                      onClick={() => selectTrigger(triggerCategory, event)}
                      className="flex h-9 w-full items-center rounded-lg px-2 text-left text-[13px] text-content/75 hover:bg-content/5 hover:text-content"
                    >
                      {event.label}
                    </button>
                  ))}
                </Popover>
              ) : null}
            </section>

            <section>
              <SectionTitle>Instructions</SectionTitle>
              <div className="relative mt-3 rounded-md border border-content/10 bg-content/3 backdrop-blur-sm has-focus:border-content/20">
                <PromptField
                  value={draft.prompt}
                  harness={draft.harness}
                  cwd={draft.cwd}
                  onChange={(prompt) => update("prompt", prompt)}
                />
                <div className="flex items-center gap-1 px-2 pb-2">
                  <div className="flex min-w-0 flex-1 items-center overflow-x-auto">
                    <div className="flex shrink-0 items-center gap-1">
                      <ModelPicker
                        harness={draft.harness}
                        model={draft.model}
                        values={draft.modelSettings}
                        hideSettings={controlsBeside}
                        onChange={(harness, model) =>
                          onChange({ ...draft, harness, model })
                        }
                        onSettingsChange={(modelSettings) =>
                          update("modelSettings", modelSettings)
                        }
                      />
                      {controlsBeside ? (
                        <ModelControlPills
                          harness={draft.harness}
                          model={draft.model}
                          values={draft.modelSettings}
                          onSettingsChange={(modelSettings) =>
                            update("modelSettings", modelSettings)
                          }
                        />
                      ) : null}
                      {draft.harness !== "fx" ? (
                        <AccessPicker
                          value={draft.runtimeMode}
                          onChange={(runtimeMode) =>
                            update("runtimeMode", runtimeMode)
                          }
                        />
                      ) : null}
                    </div>
                  </div>
                </div>
              </div>
              <p className="mt-2 px-1 text-[11px] text-content/35">
                Skills, @file references, and built-in commands work here.
              </p>
            </section>

            <section>
              <SectionTitle>Session</SectionTitle>
              <div className="mt-3 divide-y divide-content/7 rounded-md border border-content/10">
                <SettingsRow
                  label="Working copy"
                  hint="This repo, or a fresh worktree"
                >
                  <SettingsSelect
                    label="Working copy"
                    value={
                      draft.workspaceMode === "worktree"
                        ? "worktree"
                        : "current"
                    }
                    options={WORKSPACE_OPTIONS}
                    onChange={(value) =>
                      onChange({
                        ...draft,
                        workspaceMode: value as "current" | "worktree",
                        worktreeCwd: "",
                        reuseSession: value === "current" && draft.reuseSession,
                      })
                    }
                  />
                </SettingsRow>
                <SettingsRow
                  label="Conversation"
                  hint="New chat, or continue the last run"
                >
                  <SettingsSelect
                    label="Conversation"
                    value={draft.reuseSession ? "reuse" : "fresh"}
                    disabled={draft.workspaceMode === "worktree"}
                    options={CONVERSATION_OPTIONS}
                    onChange={(value) =>
                      update("reuseSession", value === "reuse")
                    }
                  />
                </SettingsRow>
                <SettingsRow
                  label="Session folder"
                  hint="Where runs appear in the sidebar"
                >
                  <SearchableSelect
                    variant="pill"
                    searchable={folderOptions.length > 6}
                    align="end"
                    label="Session folder"
                    value={draft.sessionFolderId}
                    options={folderOptions}
                    onChange={(value) => update("sessionFolderId", value)}
                  />
                </SettingsRow>
              </div>
            </section>

            <details className="rounded-md border border-content/10">
              <summary className="flex min-h-14 cursor-default list-none items-center justify-between gap-3 px-4 active:opacity-75">
                <span>
                  <span className="block text-[13px] font-medium text-content/75">
                    Advanced
                  </span>
                  <span className="mt-0.5 block text-[11px] text-content/40">
                    Catch-up window for missed runs
                  </span>
                </span>
                <ChevronDown className="size-3.5 text-content/40" />
              </summary>
              <div className="divide-y divide-content/7 border-t border-content/8">
                <SettingsRow
                  label="Missed-run grace"
                  hint="Catch up if a scheduled run was missed"
                >
                  <SettingsSelect
                    label="Missed-run grace"
                    value={String(draft.missedRunGraceMinutes)}
                    options={GRACE_OPTIONS}
                    onChange={(value) =>
                      update("missedRunGraceMinutes", Number(value))
                    }
                  />
                </SettingsRow>
              </div>
            </details>
          </div>
        ) : (
          <section className="mx-auto w-full max-w-5xl px-8 py-5 pb-10">
            <SectionTitle>Run history</SectionTitle>
            {runs.length > 0 ? (
              <div className="mt-3 overflow-hidden rounded-md border border-content/10">
                <div className={`${RUN_GRID} h-10 text-[11px] text-content/40`}>
                  <span>Trigger</span>
                  <span>Triggered</span>
                  <span>Status</span>
                  <span className="text-right">Duration</span>
                </div>
                <ul className="divide-y divide-content/8 border-t border-content/8">
                  {runs.slice(0, 100).map((run) => (
                    <RunRow
                      key={run.id}
                      run={run}
                      draft={draft}
                      onOpenSession={onOpenSession}
                    />
                  ))}
                </ul>
              </div>
            ) : (
              <div className="mt-3 rounded-md border border-dashed border-content/10 px-4 py-16 text-center text-[12px] text-content/40">
                This automation has not run yet.
              </div>
            )}
          </section>
        )}
      </div>
    </form>
  );
}

function PromptField({
  value,
  harness,
  cwd,
  onChange,
}: {
  value: string;
  harness: AutomationDraft["harness"];
  cwd: string;
  onChange: (value: string) => void;
}) {
  return (
    <SkillPromptField
      value={value}
      harness={harness}
      cwd={cwd}
      onChange={onChange}
    />
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <h2 className="px-1 text-[12px] font-medium text-content/50">{children}</h2>
  );
}

function SettingsRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-2.5">
      <span className="min-w-0">
        <span className="block text-[12px] text-content/75">{label}</span>
        {hint ? (
          <span className="mt-0.5 block text-[11px] leading-snug text-content/40">
            {hint}
          </span>
        ) : null}
      </span>
      <span className="shrink-0">{children}</span>
    </div>
  );
}

function SettingsSelect({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string;
  value: string;
  options: readonly { value: string; label: string }[];
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  return (
    <SearchableSelect
      variant="pill"
      searchable={false}
      align="end"
      label={label}
      value={value}
      options={options}
      onChange={onChange}
      disabled={disabled}
    />
  );
}

function PageTab({
  id,
  controls,
  label,
  selected,
  onSelect,
}: {
  id: string;
  controls: string;
  label: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      id={id}
      type="button"
      role="tab"
      aria-selected={selected}
      aria-controls={controls}
      onClick={onSelect}
      className={`relative flex h-9 items-center text-[12px] leading-none ${
        selected ? "text-content" : "text-content/50 hover:text-content"
      }`}
    >
      {label}
      {selected ? (
        <span className="absolute inset-x-0 bottom-0 h-0.5 bg-content" />
      ) : null}
    </button>
  );
}

function ToggleSwitch({
  label,
  on,
  onChange,
  compact = false,
}: {
  label: string;
  on: boolean;
  onChange: (on: boolean) => void;
  compact?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={label}
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={`relative shrink-0 rounded-full transition-colors duration-150 ease-out active:scale-[0.96] ${
        compact ? "h-4 w-7" : "h-5 w-9"
      } ${on ? "bg-accent" : "bg-content/20"}`}
    >
      <span
        className={`absolute top-0.5 left-0.5 rounded-full bg-white transition-transform duration-150 ease-out ${
          compact
            ? `size-3 ${on ? "translate-x-3" : "translate-x-0"}`
            : `size-4 ${on ? "translate-x-4" : "translate-x-0"}`
        }`}
      />
    </button>
  );
}

const WORKSPACE_OPTIONS = [
  { value: "current", label: "Current" },
  { value: "worktree", label: "Fresh worktree" },
] as const;

const CONVERSATION_OPTIONS = [
  { value: "fresh", label: "Start fresh" },
  { value: "reuse", label: "Continue last" },
] as const;

const GRACE_OPTIONS = [
  { value: "0", label: "Do not catch up" },
  { value: "30", label: "30 minutes" },
  { value: "120", label: "2 hours" },
  { value: "720", label: "12 hours" },
  { value: "1440", label: "24 hours" },
] as const;

const TEMPLATE_ICONS: Record<AutomationTemplateIcon, typeof Search> = {
  search: Search,
  alert: AlertCircle,
  file: File,
  check: CheckCircle,
  lock: Lock,
  pr: GitPullRequest,
  inbox: Inbox,
  gauge: Gauge,
  terminal: Terminal,
  note: StickyNote,
};

const TRIGGER_CATEGORIES: readonly {
  value: AutomationTriggerKind;
  label: string;
}[] = [
  { value: "time", label: "Scheduled" },
  { value: "github", label: "GitHub" },
  { value: "linear", label: "Linear" },
  { value: "gitlab", label: "GitLab" },
  { value: "azuredevops", label: "Azure DevOps" },
];

type TriggerEvent = {
  value: string;
  label: string;
};

const TRIGGER_EVENTS: Record<AutomationTriggerKind, readonly TriggerEvent[]> = {
  time: [
    { value: "hourly", label: "Hourly" },
    { value: "daily", label: "Daily" },
    { value: "weekdays", label: "Weekdays" },
    { value: "weekly", label: "Weekly" },
  ],
  github: [
    { value: "draft_opened", label: "Draft opened" },
    { value: "pull_request_opened", label: "Pull request opened" },
    { value: "issue_opened", label: "Issue opened" },
  ],
  linear: [{ value: "issue_created", label: "Issue created" }],
  gitlab: [
    { value: "merge_request_opened", label: "Merge request opened" },
    { value: "issue_opened", label: "Issue opened" },
  ],
  azuredevops: [
    { value: "pull_request_appeared", label: "Pull request appeared" },
    { value: "work_item_appeared", label: "Work item appeared" },
  ],
};

function TriggerMark({
  kind,
  className,
}: {
  kind: AutomationTriggerKind;
  className?: string;
}) {
  if (kind === "time") {
    return <Clock className={className} strokeWidth={1.75} />;
  }
  return <InboxProviderMark provider={kind} className={className} />;
}

function triggerName(kind: AutomationTriggerKind): string {
  return (
    TRIGGER_CATEGORIES.find((option) => option.value === kind)?.label ?? kind
  );
}

function findTriggerEvent(
  kind: AutomationTriggerKind,
  value: string,
): TriggerEvent | undefined {
  return TRIGGER_EVENTS[kind].find((event) => event.value === value);
}

function triggerLabel(automation: Automation): string {
  const triggers = automationTriggers(automation);
  if (triggers.length === 0) return "No trigger";
  const first = triggers[0]!;
  const label =
    first.kind === "time"
      ? automationScheduleLabel(first)
      : (findTriggerEvent(first.kind, first.event)?.label ??
        triggerName(first.kind));
  return triggers.length > 1 ? `${label} +${triggers.length - 1}` : label;
}

function TriggerRow({
  trigger,
  cwd,
  onChange,
  onRemove,
}: {
  trigger: AutomationTrigger;
  cwd: string;
  onChange: (trigger: AutomationTrigger) => void;
  onRemove: () => void;
}) {
  const [branches, setBranches] = useState<string[]>([]);
  const projectPath = looksLikeProject(cwd) ? cwd : "";
  const push = trigger.event === "push_to_branch";
  useEffect(() => {
    if (!projectPath || !push) {
      setBranches([]);
      return;
    }
    let cancelled = false;
    void gitBranches(projectPath).then(
      (info) => {
        if (!cancelled) setBranches(info.branches.map((branch) => branch.name));
      },
      () => {
        if (!cancelled) setBranches([]);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [projectPath, push]);
  const branchOptions = branches.map((name) => ({ value: name, label: name }));
  if (
    trigger.branch &&
    !branchOptions.some((option) => option.value === trigger.branch)
  ) {
    branchOptions.unshift({ value: trigger.branch, label: trigger.branch });
  }
  const nextAt = trigger.kind === "time" ? nextAutomationRunAt(trigger) : 0;

  return (
    <div className="group flex min-h-10 items-center gap-2.5 py-1">
      <TriggerMark
        kind={trigger.kind}
        className="size-3.5 shrink-0 text-content/45"
      />
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-1.5 gap-y-1 text-[13px] text-content/70">
        {trigger.kind === "time" ? (
          <TimeTriggerSentence
            trigger={trigger}
            nextAt={nextAt}
            onChange={onChange}
          />
        ) : (
          <EventTriggerSentence
            trigger={trigger}
            branchOptions={branchOptions}
            projectChosen={Boolean(projectPath)}
            onChange={onChange}
          />
        )}
      </div>
      <button
        type="button"
        aria-label="Remove trigger"
        onClick={onRemove}
        className="grid size-7 shrink-0 place-items-center rounded-md text-content/35 opacity-0 transition-opacity duration-150 ease-out group-hover:opacity-100 group-focus-within:opacity-100 hover:bg-content/8 hover:text-content focus-visible:opacity-100"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

function TimeTriggerSentence({
  trigger,
  nextAt,
  onChange,
}: {
  trigger: AutomationTrigger;
  nextAt: number;
  onChange: (trigger: AutomationTrigger) => void;
}) {
  const dayOptions = AUTOMATION_WEEKDAYS.map((label, value) => ({
    value: String(value),
    label,
  }));
  const minuteOptions = [0, 15, 30, 45].map((value) => ({
    value: String(value),
    label: `:${String(value).padStart(2, "0")}`,
  }));
  const prefix =
    trigger.scheduleKind === "hourly"
      ? "Every hour at"
      : trigger.scheduleKind === "daily"
        ? "Every day at"
        : trigger.scheduleKind === "weekdays"
          ? "Every weekday at"
          : "Every week on";
  return (
    <>
      <span>{prefix}</span>
      {trigger.scheduleKind === "weekly" ? (
        <>
          <TriggerPill
            label="Day"
            value={String(trigger.dayOfWeek)}
            options={dayOptions}
            onChange={(value) =>
              onChange({ ...trigger, dayOfWeek: Number(value) })
            }
          />
          <span>at</span>
        </>
      ) : null}
      {trigger.scheduleKind === "hourly" ? (
        <TriggerPill
          label="Minute"
          value={String(trigger.minute)}
          options={minuteOptions}
          onChange={(value) => onChange({ ...trigger, minute: Number(value) })}
        />
      ) : (
        <TriggerPill
          label="Time"
          value={trigger.time}
          options={timeOptions(trigger.time)}
          onChange={(value) => onChange({ ...trigger, time: value })}
        />
      )}
      <span className="text-content/45">{gmtOffsetLabel()}</span>
      <span className="ml-1 text-content/35">{nextRunPreview(nextAt)}</span>
    </>
  );
}

function EventTriggerSentence({
  trigger,
  branchOptions,
  projectChosen,
  onChange,
}: {
  trigger: AutomationTrigger;
  branchOptions: { value: string; label: string }[];
  projectChosen: boolean;
  onChange: (trigger: AutomationTrigger) => void;
}) {
  const stem =
    trigger.event === "push_to_branch"
      ? "Push"
      : (findTriggerEvent(trigger.kind, trigger.event)?.label ?? trigger.event);
  const push = trigger.event === "push_to_branch";
  return (
    <>
      <span>{stem}</span>
      {push ? (
        <>
          <span>on</span>
          <TriggerPill
            label="Branch"
            value={trigger.branch}
            options={branchOptions}
            placeholder="Select a branch"
            disabled={!projectChosen}
            emptyLabel={
              projectChosen ? "No branches found" : "Choose a project first"
            }
            onChange={(value) => onChange({ ...trigger, branch: value })}
          />
        </>
      ) : null}
      <span>by</span>
      <TriggerPill
        label="Actor"
        value={trigger.actor || "anyone"}
        options={[{ value: "anyone", label: "Anyone" }]}
        onChange={(value) => onChange({ ...trigger, actor: value })}
      />
    </>
  );
}

function TriggerPill({
  label,
  value,
  options,
  onChange,
  placeholder,
  disabled,
  emptyLabel,
}: {
  label: string;
  value: string;
  options: readonly { value: string; label: string; keywords?: string }[];
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  emptyLabel?: string;
}) {
  return (
    <SearchableSelect
      label={label}
      value={value}
      options={options}
      onChange={onChange}
      placeholder={placeholder}
      disabled={disabled}
      emptyLabel={emptyLabel}
      variant="pill"
      searchable={options.length > 8}
    />
  );
}

function timeOptions(current: string) {
  const options = Array.from({ length: 24 }, (_, hour) => {
    const prefix = String(hour).padStart(2, "0");
    return [
      { value: `${prefix}:00`, label: `${prefix}:00` },
      { value: `${prefix}:30`, label: `${prefix}:30` },
    ];
  }).flat();
  if (current && !options.some((option) => option.value === current)) {
    options.unshift({ value: current, label: current });
  }
  return options;
}
