import { useEffect, useRef, useState } from "react";
import { Check, ChevronRight, Folder, Minus } from "../../../shared/ui/icons";
import { NotificationMuteControl } from "./NotificationMuteControl";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { ProjectLogoIcon } from "../../projects/ui/ProjectLogoIcon";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import { useTabGroupLogos } from "../../projects/hooks/useTabGroupLogos";
import { useProjectNotificationPreferences } from "../hooks/useProjectNotificationPreferences";
import { useNotificationProjects } from "../hooks/useNotificationProjects";
import {
  NOTIFICATION_CATEGORIES,
  isProjectMuted,
  loadNotificationPreferences,
  updateNotificationPreferences,
  type NotificationCategory,
} from "../model/notificationPreferences";
import { pathKey, projectKey, projectName } from "../../../shared/lib/paths";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";
import type { RecentProject } from "../../projects/model/recents";

type Props = {
  cwd: string;
  recents?: RecentProject[];
  notificationProjectPath?: string | null;
  notificationSettingsRequest?: number;
  highlighted?: boolean;
};

export function ProjectNotificationSettings({
  cwd,
  recents = [],
  notificationProjectPath = null,
  notificationSettingsRequest = 0,
  highlighted = false,
}: Props) {
  const notificationProjects = useNotificationProjects([
    cwd,
    notificationProjectPath ?? "",
    ...recents.map((project) => project.path),
  ]);
  const projects = [...notificationProjects.projects].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const preferences = useProjectNotificationPreferences();
  const groupLogos = useTabGroupLogos();
  const groupColors = loadTabGroupColors();
  const groupCustomColors = loadTabGroupCustomColors();
  const groupMascots = loadTabGroupMascots();
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [selecting, setSelecting] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const selectedIds = selected.filter((id) =>
    projects.some((project) => project.id === id),
  );
  const targetCard = useRef<HTMLFieldSetElement>(null);
  const focusedRequest = useRef<{ path: string; request: number } | null>(null);
  const targetId = notificationProjectPath
    ? projects.find((project) =>
        project.paths.some(
          (path) => pathKey(path) === pathKey(notificationProjectPath),
        ),
      )?.id
    : undefined;
  useEffect(() => {
    if (!notificationProjectPath) {
      focusedRequest.current = null;
      return;
    }
    if (
      (focusedRequest.current?.path === notificationProjectPath &&
        focusedRequest.current.request === notificationSettingsRequest) ||
      !targetCard.current
    )
      return;
    setExpanded(targetId ?? null);
    targetCard.current.scrollIntoView?.({ block: "nearest" });
    targetCard.current.focus({ preventScroll: true });
    focusedRequest.current = {
      path: notificationProjectPath,
      request: notificationSettingsRequest,
    };
  }, [notificationProjectPath, notificationSettingsRequest, targetId]);

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
      className="@container/notifications"
    >
      <div className="flex flex-wrap items-end gap-4 pb-2.5">
        <div className="min-w-[min(100%,240px)] flex-1">
          <h2 className="text-[13px] font-semibold text-content">
            Project notifications
          </h2>
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            Choose sounds, banners and sidebar indicators by category. Mute
            pauses them without changing your choices. Unread items stay marked
            in Inbox.
          </p>
        </div>
        {projects.length ? (
          <div className="shrink-0 pb-0.5">
            <SecondaryButton
              type="button"
              aria-pressed={selecting}
              onClick={() => {
                setSelecting(!selecting);
                setSelected([]);
              }}
            >
              {selecting ? "Done" : "Select projects"}
            </SecondaryButton>
          </div>
        ) : null}
      </div>
      <div
        className={`overflow-hidden rounded-xl border bg-content/3 transition-colors ${
          highlighted ? "border-accent/60" : "border-content/10"
        }`}
      >
        {error ? (
          <p role="alert" className="px-4 py-3.5 text-[12px] text-red-400">
            {error}
          </p>
        ) : null}
        {projects.length === 0 ? (
          <p
            role="status"
            className="px-4 py-3.5 text-[12px] leading-relaxed text-content/45"
          >
            Open a project or connect an Inbox provider to configure its notifications.
          </p>
        ) : null}
        {projects.length ? (
          <>
            {selecting ? (
              <div className="flex min-h-9 flex-wrap items-center justify-between gap-3 border-b border-content/5 px-4 py-3.5">
                <label className="flex cursor-pointer items-center gap-2.5 text-[12px] text-content/55 hover:text-content/80">
                  <ProjectSelection
                    label="Select all projects"
                    checked={selectedIds.length === projects.length}
                    mixed={
                      selectedIds.length > 0 &&
                      selectedIds.length < projects.length
                    }
                    onChange={(checked) =>
                      setSelected(
                        checked ? projects.map((project) => project.id) : [],
                      )
                    }
                  />
                  {selectedIds.length
                    ? `${selectedIds.length} selected`
                    : "Select all projects"}
                </label>
                {selectedIds.length ? (
                  <div role="group" aria-label="Mute selected projects">
                    <NotificationMuteControl projectIds={selectedIds} />
                  </div>
                ) : null}
              </div>
            ) : null}
            <div>
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
                  (category) => {
                    if (project.kind === "linear" || project.kind === "jira")
                      return category.id === "issues";
                    if (project.kind === "local")
                      return !["pullRequests", "issues"].includes(category.id);
                    return true;
                  },
                );
                const enabledCount = categories.filter(
                  (category) =>
                    !preferences[project.id]?.disabled.includes(category.id),
                ).length;
                const muted = isProjectMuted(
                  preferences[project.id] ?? { disabled: [] },
                );
                const isExpanded = expanded === project.id;
                const panelId = `notification-categories-${encodeURIComponent(project.id)}`;
                const muteHintId = `${panelId}-mute-hint`;
                return (
                  <fieldset
                    key={project.id}
                    ref={project.id === targetId ? targetCard : undefined}
                    tabIndex={-1}
                    className="min-w-0 border-b border-content/5 outline-none last:border-b-0 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent/50"
                  >
                    <legend className="sr-only">{project.name}</legend>
                    <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 px-4 py-3.5">
                      <div className="flex min-w-[min(100%,200px)] flex-1 items-center gap-3">
                        {selecting ? (
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
                        ) : null}
                        <button
                          type="button"
                          aria-label={`Notification categories for ${project.name}`}
                          aria-expanded={isExpanded}
                          aria-controls={panelId}
                          onClick={() =>
                            setExpanded(isExpanded ? null : project.id)
                          }
                          className="group flex min-w-0 flex-1 items-center gap-3 rounded-md text-left focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-accent"
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
                              className="truncate text-[13px] font-medium text-content group-hover:text-content/75"
                              title={`${project.name} (${project.detail})`}
                            >
                              {project.name}
                            </p>
                            <p className="mt-1 text-[12px] leading-relaxed text-content/45">
                              {project.kind === "local"
                                ? "Local project · "
                                : ""}
                              {muted
                                ? "All notifications paused"
                                : enabledCount === categories.length
                                  ? "All categories enabled"
                                  : `${enabledCount} of ${categories.length} enabled`}
                            </p>
                          </div>
                          <ChevronRight
                            className={`size-3.5 shrink-0 text-content/40 ${isExpanded ? "rotate-90" : ""}`}
                            aria-hidden="true"
                          />
                        </button>
                      </div>
                      <div className="ml-auto max-w-full">
                        <NotificationMuteControl projectIds={[project.id]} />
                      </div>
                    </div>
                    <div
                      id={panelId}
                      hidden={!isExpanded}
                      className="border-t border-content/5 px-4"
                    >
                      <div
                        className={
                          selecting
                            ? "@[400px]/notifications:pl-14"
                            : "@[400px]/notifications:pl-7"
                        }
                      >
                        {muted ? (
                          <p
                            id={muteHintId}
                            role="status"
                            className="pt-3.5 text-[12px] leading-relaxed text-content/45"
                          >
                            Your category choices apply when notifications
                            resume. You can edit them while muted.
                          </p>
                        ) : null}
                        {categories.map((category) => (
                          <label
                            key={category.id}
                            className="flex min-h-11 cursor-pointer items-center justify-between gap-6 border-b border-content/5 py-3.5 text-[13px] text-content last:border-b-0 hover:text-content/75"
                          >
                            <span>{category.label}</span>
                            <span className="relative flex shrink-0">
                              <input
                                type="checkbox"
                                role="switch"
                                aria-label={`${category.label} for ${project.name}`}
                                aria-describedby={
                                  muted ? muteHintId : undefined
                                }
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
      </div>
    </section>
  );
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
