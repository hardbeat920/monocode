import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Check, ChevronRight, Folder, Minus } from "../chrome/icons";
import { NotificationMuteControl } from "../chrome/NotificationMuteControl";
import { ProjectLogoIcon } from "../chrome/ProjectLogoIcon";
import { ProjectMascot } from "../chrome/ProjectMascot";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import { useProjectNotificationPreferences } from "../hooks/useProjectNotificationPreferences";
import { useNotificationProjects } from "../hooks/useNotificationProjects";
import {
  NOTIFICATION_CATEGORIES,
  loadNotificationPreferences,
  updateNotificationPreferences,
  type NotificationCategory,
} from "../lib/notificationPreferences";
import { pathKey, projectKey, projectName } from "../lib/paths";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../lib/tabGroups";
import { loadSoundsEnabled, SOUNDS_CHANGE_EVENT } from "../lib/sounds";
import {
  loadNotificationsEnabled,
  NOTIFICATIONS_CHANGE_EVENT,
} from "../lib/notifications";
import type { RecentProject } from "../lib/recents";

type Props = {
  cwd: string;
  recents?: RecentProject[];
  notificationProjectPath?: string | null;
};

export function ProjectNotificationSettings({
  cwd,
  recents = [],
  notificationProjectPath = null,
}: Props) {
  const discovery = useNotificationProjects([
    cwd,
    notificationProjectPath ?? "",
    ...recents.map((project) => project.path),
  ]);
  const projects = [...discovery.projects].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const preferences = useProjectNotificationPreferences();
  const groupLogos = useTabGroupLogos();
  const groupColors = loadTabGroupColors();
  const groupCustomColors = loadTabGroupCustomColors();
  const groupMascots = loadTabGroupMascots();
  const [error, setError] = useState<string | null>(null);
  const loading = discovery.loading;
  const [selected, setSelected] = useState<string[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);
  const selectedIds = selected.filter((id) =>
    projects.some((project) => project.id === id),
  );
  const soundsEnabled = useSyncExternalStore(
    subscribeChannels,
    loadSoundsEnabled,
    loadSoundsEnabled,
  );
  const desktopEnabled = useSyncExternalStore(
    subscribeChannels,
    loadNotificationsEnabled,
    loadNotificationsEnabled,
  );
  const targetCard = useRef<HTMLFieldSetElement>(null);
  const focusedPath = useRef<string | null>(null);
  const targetId = notificationProjectPath
    ? projects.find((project) =>
        project.paths.some(
          (path) => pathKey(path) === pathKey(notificationProjectPath),
        ),
      )?.id
    : undefined;
  useEffect(() => {
    if (!notificationProjectPath) {
      focusedPath.current = null;
      return;
    }
    if (focusedPath.current === notificationProjectPath || !targetCard.current)
      return;
    setExpanded(targetId ?? null);
    targetCard.current.scrollIntoView?.({ block: "nearest" });
    targetCard.current.focus({ preventScroll: true });
    focusedPath.current = notificationProjectPath;
  }, [notificationProjectPath, targetId]);

  function setCategory(
    projectId: string,
    category: NotificationCategory,
    enabled: boolean,
  ) {
    try {
      const disabled = loadNotificationPreferences()[projectId]?.disabled ?? [];
      updateNotificationPreferences([projectId], {
        disabled: enabled
          ? disabled.filter((id) => id !== category)
          : [...disabled, category],
      });
      setError(null);
    } catch {
      setError("Could not save notification preferences. Please try again.");
    }
  }

  return (
    <section
      id="settings-project-notifications"
      aria-label="Project notifications"
    >
      <h2 className="text-[15px] font-semibold text-content">
        Project notifications
      </h2>
      <p className="mt-1 text-[12px] leading-relaxed text-content/45">
        Choose which activity can send sounds and banners. Activity stays
        available in MonoCode.
      </p>
      {!soundsEnabled || !desktopEnabled ? (
        <div
          role="status"
          className="mt-3 rounded-md bg-content/5 px-3 py-2 text-[12px] leading-relaxed text-content/55"
        >
          {!soundsEnabled ? <p>Sounds are off globally.</p> : null}
          {!desktopEnabled ? (
            <p>Desktop notifications are off globally.</p>
          ) : null}
          <p>
            Enable them in General to receive the notifications you choose here.
          </p>
        </div>
      ) : null}
      {error || discovery.error ? (
        <p role="alert" className="mt-3 text-[12px] text-red-400">
          {error ?? discovery.error}
        </p>
      ) : null}
      {projects.length === 0 ? (
        <p role="status" className="py-4 text-[12px] text-content/45">
          {loading
            ? "Loading projects…"
            : "Open a project or connect an Inbox provider to configure its notifications."}
        </p>
      ) : null}
      {projects.length ? (
        <>
          <div className="mt-3 flex min-h-9 flex-wrap items-center justify-between gap-3 px-1 py-1.5">
            <label className="flex cursor-pointer items-center gap-2.5 text-[12px] text-content/55 hover:text-content/80">
              <ProjectSelection
                label="Select all projects"
                checked={selectedIds.length === projects.length}
                mixed={
                  selectedIds.length > 0 && selectedIds.length < projects.length
                }
                onChange={(checked) =>
                  setSelected(
                    checked ? projects.map((project) => project.id) : [],
                  )
                }
              />
              {selectedIds.length
                ? `${selectedIds.length} selected`
                : "Select projects to mute together"}
            </label>
            {selectedIds.length ? (
              <div role="group" aria-label="Mute selected projects">
                <NotificationMuteControl projectIds={selectedIds} />
              </div>
            ) : (
              <span className="text-[11px] text-content/35">
                {projects.length}{" "}
                {projects.length === 1 ? "project" : "projects"}
              </span>
            )}
          </div>
          <div className="mt-1 border-y border-content/10">
            {projects.map((project) => {
              const path =
                project.paths.find(
                  (path) =>
                    pathKey(path) === pathKey(notificationProjectPath || cwd),
                ) ?? project.paths[0];
              const key = path ? projectKey(path) : null;
              const seed = path ? projectName(path) : project.name;
              const logoPath = key
                ? resolveTabGroupLogo(key, groupLogos)
                : null;
              const categories = NOTIFICATION_CATEGORIES.filter(
                (category) =>
                  project.kind !== "linear" || category.id === "issues",
              );
              const enabledCount = categories.filter(
                (category) =>
                  !preferences[project.id]?.disabled.includes(category.id),
              ).length;
              const isExpanded = expanded === project.id;
              const panelId = `notification-categories-${encodeURIComponent(project.id)}`;
              return (
                <fieldset
                  key={project.id}
                  ref={project.id === targetId ? targetCard : undefined}
                  tabIndex={-1}
                  className="min-w-0 border-t border-content/5 first:border-t-0 outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent/50"
                >
                  <legend className="sr-only">{project.name}</legend>
                  <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 px-1 py-2">
                    <div className="flex min-w-[min(100%,240px)] flex-1 items-center gap-3">
                      <ProjectSelection
                        label={`Select ${project.name}`}
                        checked={selectedIds.includes(project.id)}
                        onChange={(checked) =>
                          setSelected((current) =>
                            checked
                              ? [...current, project.id]
                              : current.filter((id) => id !== project.id),
                          )
                        }
                      />
                      <button
                        type="button"
                        aria-label={`Notification categories for ${project.name}`}
                        aria-expanded={isExpanded}
                        aria-controls={panelId}
                        onClick={() =>
                          setExpanded(isExpanded ? null : project.id)
                        }
                        className="flex min-w-0 flex-1 items-center gap-3 rounded-md py-1.5 text-left hover:bg-content/3 focus-visible:outline-2 focus-visible:outline-accent"
                      >
                        <span className="grid size-4 shrink-0 place-items-center">
                          {logoPath ? (
                            <ProjectLogoIcon
                              path={logoPath}
                              className="size-4 rounded-sm"
                              imageClassName="size-4"
                            />
                          ) : key ? (
                            <ProjectMascot
                              project={seed}
                              color={resolveTabGroupColor(
                                key,
                                groupColors,
                                groupCustomColors,
                                seed,
                              )}
                              name={resolveTabGroupMascot(key, groupMascots)}
                              className="size-3"
                            />
                          ) : (
                            <Folder
                              className="size-4 text-content/40"
                              aria-hidden="true"
                            />
                          )}
                        </span>
                        <div className="min-w-0 flex-1">
                          <p
                            className="truncate text-[13px] font-medium text-content"
                            title={`${project.name} · ${project.detail}`}
                          >
                            {project.name}
                          </p>
                        </div>
                        <span className="shrink-0 text-[11px] font-normal text-content/40">
                          {enabledCount === categories.length
                            ? "All activity"
                            : `${enabledCount} of ${categories.length} enabled`}
                        </span>
                        <ChevronRight
                          className={`mr-1 size-3 shrink-0 text-content/40 ${isExpanded ? "rotate-90" : ""}`}
                          aria-hidden="true"
                        />
                      </button>
                    </div>
                    <NotificationMuteControl projectIds={[project.id]} />
                  </div>
                  <div id={panelId} hidden={!isExpanded}>
                    <div className="grid gap-x-8 pb-3 pl-8 pr-1 sm:grid-cols-2">
                      {categories.map((category) => (
                        <label
                          key={category.id}
                          className="flex min-h-8 cursor-pointer items-center justify-between gap-4 rounded-md px-2 text-[12px] text-content/70 hover:bg-content/3 hover:text-content"
                        >
                          <span>{category.label}</span>
                          <span className="relative flex shrink-0">
                            <input
                              type="checkbox"
                              role="switch"
                              aria-label={`${category.label} for ${project.name}`}
                              checked={
                                !preferences[project.id]?.disabled.includes(
                                  category.id,
                                )
                              }
                              onChange={(event) =>
                                setCategory(
                                  project.id,
                                  category.id,
                                  event.target.checked,
                                )
                              }
                              className="peer sr-only"
                            />
                            <span
                              className="relative h-5 w-9 rounded-full bg-content/20 transition-colors peer-checked:bg-accent peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent motion-reduce:transition-none"
                              aria-hidden="true"
                            />
                            <span
                              className="pointer-events-none absolute left-0.5 top-0.5 size-4 rounded-full bg-white transition-transform peer-checked:translate-x-4 motion-reduce:transition-none"
                              aria-hidden="true"
                            />
                          </span>
                        </label>
                      ))}
                    </div>
                  </div>
                </fieldset>
              );
            })}
          </div>
        </>
      ) : null}
    </section>
  );
}

function subscribeChannels(listener: () => void) {
  window.addEventListener(SOUNDS_CHANGE_EVENT, listener);
  window.addEventListener(NOTIFICATIONS_CHANGE_EVENT, listener);
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(SOUNDS_CHANGE_EVENT, listener);
    window.removeEventListener(NOTIFICATIONS_CHANGE_EVENT, listener);
    window.removeEventListener("storage", listener);
  };
}

function ProjectSelection({
  label,
  checked,
  mixed = false,
  onChange,
}: {
  label: string;
  checked: boolean;
  mixed?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <span className="relative flex size-4 shrink-0 items-center justify-center">
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        ref={(input) => {
          if (input) input.indeterminate = mixed;
        }}
        onChange={(event) => onChange(event.target.checked)}
        className="peer size-4 cursor-pointer appearance-none rounded border border-content/20 bg-transparent checked:border-accent checked:bg-accent indeterminate:border-accent indeterminate:bg-accent hover:border-content/40 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
      />
      {mixed ? (
        <Minus
          className="pointer-events-none absolute size-3 text-white"
          aria-hidden="true"
        />
      ) : (
        <Check
          className="pointer-events-none absolute hidden size-3 text-white peer-checked:block"
          aria-hidden="true"
        />
      )}
    </span>
  );
}
