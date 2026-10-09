import { useEffect, useState, type ReactNode } from "react";
import type { HarnessId, RuntimeMode } from "../../sessions/model/session";
import { runtimeModesFor } from "../../sessions/model/models";
import { AccessPicker } from "../../sessions/ui/AccessPicker";
import { ModelPicker, ModelSettingRows } from "../../sessions/ui/ModelPicker";
import type { MonoLook, MonoState } from "../model/mono";
import {
  loadMonoFiles,
  subscribeMonoFiles,
  type MonoFiles,
} from "../model/monoFiles";
import { IconButton } from "../../../app/shell/TitleBar";
import { Plus } from "../../../shared/ui/icons";
import { HABITS_MAX } from "../model/monoHabits";
import { memoryLines } from "../model/monoMemory";
import { HabitPage } from "./HabitPage";
import { NewHabitPage } from "./NewHabitPage";
import { MonoProjects } from "./MonoProjects";
import { MonoPreferencesPage } from "./MonoPreferencesPage";
import { MonoSettingsPage } from "./MonoSettingsPage";
import { habitActions, HabitsList, useHabits } from "./MonoHabits";
import { MemoryPage, SoulPage } from "./MonoFilePages";
import { PageHeader, Property } from "./monoPanelParts";
import { PanelStack, type StackPage } from "./PanelStack";
import { MonoSidebar, MonoSidebarHeader } from "./MonoSidebar";

/** A page opened directly from Details, or one habit inside its list. */
type Route =
  | { kind: "habits" | "soul" | "memory" | "settings" | "new-habit" }
  | { kind: "habit"; id: string };

type Props = {
  open: boolean;
  monoId: string;
  /** Its conversation's folder, which the model picker reads settings from. */
  cwd: string;
  agent: MonoLook;
  state: MonoState;
  harness: HarnessId;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: RuntimeMode;
  busy?: boolean;
  onModelChange: (harness: HarnessId, model: string) => void;
  onModelSettingsChange: (settings: Record<string, string>) => void;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  onClose: () => void;
  onReset?: () => Promise<void>;
  windowControls?: ReactNode;
};

/**
 * The Mono's profile, model, permissions and projects in one panel. Its habits,
 * soul, memory and settings open directly as pages that slide over it.
 */
export function MonoDetails({
  open,
  monoId,
  cwd,
  agent,
  state,
  harness,
  model,
  modelSettings,
  runtimeMode,
  busy = false,
  onModelChange,
  onModelSettingsChange,
  onRuntimeModeChange,
  onClose,
  onReset,
  windowControls,
}: Props) {
  const files = useMonoFiles(monoId, state.status);
  const habits = useHabits(monoId, state.status);
  const actions = habitActions(monoId);
  const [routes, setRoutes] = useState<Route[]>([]);
  // Another Mono starts at its own front page.
  useEffect(() => setRoutes([]), [monoId]);
  const push = (route: Route) => setRoutes((current) => [...current, route]);
  const back = () => setRoutes((current) => current.slice(0, -1));

  const pages: StackPage[] = routes.flatMap((route, depth): StackPage[] => {
    if (route.kind === "habits")
      return [
        {
          key: "habits",
          node: (
            <div className="flex min-h-0 flex-1 flex-col" data-mono-habits>
              <PageHeader title="Habits" onBack={back}>
                <IconButton
                  label={
                    (habits?.length ?? 0) >= HABITS_MAX
                      ? `At most ${HABITS_MAX} habits`
                      : "New habit"
                  }
                  disabled={!habits || habits.length >= HABITS_MAX}
                  onClick={() => push({ kind: "new-habit" })}
                >
                  <Plus className="size-3.5" strokeWidth={1.75} />
                </IconButton>
              </PageHeader>
              <div className="min-h-0 flex-1 overflow-y-auto overscroll-none px-2 py-2">
                <HabitsList
                  habits={habits}
                  actions={actions}
                  onOpen={(id) => push({ kind: "habit", id })}
                />
              </div>
            </div>
          ),
        },
      ];
    if (route.kind === "soul")
      return [
        {
          key: "soul",
          node: (
            <SoulPage
              monoId={monoId}
              agent={agent}
              files={files}
              onBack={back}
            />
          ),
        },
      ];
    if (route.kind === "memory")
      return [
        {
          key: "memory",
          node: <MemoryPage monoId={monoId} files={files} onBack={back} />,
        },
      ];
    if (route.kind === "settings")
      return [
        {
          key: "settings",
          node: (
            <MonoPreferencesPage
              monoId={monoId}
              agent={agent}
              onBack={back}
              onReset={onReset}
            />
          ),
        },
      ];
    if (route.kind === "new-habit")
      return [
        {
          key: "new-habit",
          node: <NewHabitPage monoId={monoId} onBack={back} onCreated={back} />,
        },
      ];
    if (route.kind !== "habit") return [];
    // A habit removed while open closes its page.
    const habit = habits?.find((entry) => entry.id === route.id);
    if (!habit) return [];
    return [
      {
        key: `habit:${route.id}:${depth}`,
        node: (
          <HabitPage
            habit={habit}
            color={agent.color}
            cwd={cwd}
            onBack={back}
            onRunNow={() => actions.runNow(habit.id)}
            onToggle={() => actions.toggle(habit.id)}
            onRemove={() => {
              actions.remove(habit.id);
              back();
            }}
          />
        ),
      },
    ];
  });

  return (
    <MonoSidebar
      open={open}
      kind="details"
      label={`${agent.name} details`}
      color={agent.color}
      windowControls={windowControls}
    >
      <PanelStack pages={pages}>
        {/* Pages replace this header too, keeping their back button at the top. */}
        <MonoSidebarHeader title="Details" onClose={onClose} />
        <MonoSettingsPage
          monoId={monoId}
          agent={agent}
          onOpen={(page) => push({ kind: page })}
          counts={{
            habits: habits?.length,
            memory: files ? memoryLines(files.memory).length : undefined,
          }}
        >
          <dl className="flex flex-col gap-0.5 border-t border-stroke px-4 py-3">
            <Property label="Model">
              <ModelPicker
                harness={harness}
                model={model}
                values={modelSettings}
                project={cwd}
                hideSettings
                side="bottom"
                variant="plain"
                onChange={onModelChange}
                onSettingsChange={onModelSettingsChange}
              />
            </Property>
            <ModelSettingRows
              harness={harness}
              model={model}
              values={modelSettings}
              side="bottom"
              onSettingsChange={onModelSettingsChange}
              row={({ label, control }) => (
                <Property label={label}>{control}</Property>
              )}
            />
            {runtimeModesFor(harness, model).length > 1 ? (
              <Property label="Permissions">
                <AccessPicker
                  harness={harness}
                  model={model}
                  value={runtimeMode}
                  onChange={onRuntimeModeChange}
                  busy={busy}
                  side="bottom"
                  variant="plain"
                />
              </Property>
            ) : null}
            <Property label="Projects">
              <MonoProjects monoId={monoId} projects={agent.projects} />
            </Property>
          </dl>
        </MonoSettingsPage>
      </PanelStack>
    </MonoSidebar>
  );
}

/** The agent's files, reloaded when it finishes a turn or the app regains focus. */
function useMonoFiles(
  monoId: string,
  status: MonoState["status"],
): MonoFiles | undefined {
  const [files, setFiles] = useState<MonoFiles>();
  const working = status === "working";
  useEffect(() => {
    let live = true;
    const refresh = () => {
      void loadMonoFiles(monoId)
        .then((next) => {
          if (live) setFiles(next);
        })
        .catch((error) =>
          console.warn("Could not load the Mono's files", error),
        );
    };
    refresh();
    const stop = subscribeMonoFiles(refresh);
    window.addEventListener("focus", refresh);
    return () => {
      live = false;
      stop();
      window.removeEventListener("focus", refresh);
    };
    // A turn ending is when the agent may have written to its memory.
  }, [monoId, working]);
  return files;
}
