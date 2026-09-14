import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { NotificationMuteControl } from "../chrome/NotificationMuteControl";
import { useProjectNotificationPreferences } from "../hooks/useProjectNotificationPreferences";
import { useNotificationProjects } from "../hooks/useNotificationProjects";
import {
  NOTIFICATION_CATEGORIES,
  loadNotificationPreferences,
  updateNotificationPreferences,
  type NotificationCategory,
} from "../lib/notificationPreferences";
import { pathKey } from "../lib/paths";
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
    ...recents.map(project => project.path),
  ]);
  const projects = [...discovery.projects].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const preferences = useProjectNotificationPreferences();
  const [error, setError] = useState<string | null>(null);
  const loading = discovery.loading;
  const [selected, setSelected] = useState<string[]>([]);
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
        <div className="mt-4 space-y-3 rounded-lg border border-content/10 bg-content/3 px-3 py-3">
          <label className="flex w-fit cursor-pointer items-center gap-2 text-[12px] text-content/70">
            <input
              type="checkbox"
              aria-label="Select all projects"
              checked={selectedIds.length === projects.length}
              ref={(input) => {
                if (input)
                  input.indeterminate =
                    selectedIds.length > 0 &&
                    selectedIds.length < projects.length;
              }}
              onChange={(event) =>
                setSelected(
                  event.target.checked
                    ? projects.map((project) => project.id)
                    : [],
                )
              }
              className="size-3.5 accent-accent"
            />
            {selectedIds.length
              ? `${selectedIds.length} selected`
              : "Select projects to mute together"}
          </label>
          {selectedIds.length ? (
            <div role="group" aria-label="Mute selected projects">
              <NotificationMuteControl projectIds={selectedIds} />
            </div>
          ) : null}
        </div>
      ) : null}
      <div className="mt-4 space-y-3">
        {projects.map((project) => (
          <fieldset
            key={project.id}
            ref={project.id === targetId ? targetCard : undefined}
            tabIndex={-1}
            className="rounded-lg border border-content/10 px-4 pb-4 outline-none focus:border-accent/50"
          >
            <legend className="max-w-full px-1 text-[13px] font-medium text-content">
              <label className="flex cursor-pointer items-center gap-2">
                <input
                  type="checkbox"
                  aria-label={`Select ${project.name}`}
                  checked={selectedIds.includes(project.id)}
                  onChange={(event) =>
                    setSelected((current) =>
                      event.target.checked
                        ? [...current, project.id]
                        : current.filter((id) => id !== project.id),
                    )
                  }
                  className="size-3.5 shrink-0 accent-accent"
                />
                <span className="truncate" title={project.name}>
                  {project.name}
                </span>
              </label>
            </legend>
            <p
              className="truncate text-[11px] text-content/40"
              title={project.detail}
            >
              {project.detail}
            </p>
            <div className="mt-3 grid gap-x-5 gap-y-2 sm:grid-cols-2">
              {NOTIFICATION_CATEGORIES.filter(
                (category) =>
                  project.kind !== "linear" || category.id === "issues",
              ).map((category) => (
                <label
                  key={category.id}
                  className="flex cursor-pointer items-start gap-2 text-[12px] text-content/75"
                >
                  <input
                    type="checkbox"
                    aria-label={`${category.label} for ${project.name}`}
                    checked={
                      !preferences[project.id]?.disabled.includes(category.id)
                    }
                    onChange={(event) =>
                      setCategory(project.id, category.id, event.target.checked)
                    }
                    className="mt-0.5 size-3.5 shrink-0 accent-accent"
                  />
                  {category.label}
                </label>
              ))}
            </div>
            <div className="mt-4 border-t border-content/5 pt-3">
              <NotificationMuteControl projectIds={[project.id]} />
            </div>
          </fieldset>
        ))}
      </div>
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
