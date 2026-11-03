import { useLayoutEffect, useMemo, useRef, type RefObject } from "react";
import { basename } from "../../platform/tauri/fs";
import { MOD } from "../../platform/tauri/platform";
import { useProjectAppearance } from "../../features/projects/hooks/useProjectAppearance";
import {
  projectRailItems,
  sameProjectPath,
  type RecentProject,
} from "../../features/projects/model/recents";
import { resolveTabGroupLabel } from "../../features/workspace/model/tabGroups";
import { projectKey, projectName } from "../../shared/lib/paths";
import { Plus, Settings } from "../../shared/ui/icons";
import { IconButton } from "./TitleBar";

/** How far a switched-in project slides, and for how long. */
const SLIDE_DISTANCE = 28;
const SLIDE_MS = 320;
const SLIDE_EASING = "cubic-bezier(0.32, 0.72, 0, 1)";

/**
 * The projects in the order `Project: Previous` / `Project: Next` step
 * through, so the dots and the shortcut agree on what is next.
 */
export function useProjectSpaces(recents: RecentProject[], cwd: string) {
  return useMemo(
    () => projectRailItems(recents, cwd).map((project) => project.path),
    [recents, cwd],
  );
}

/**
 * Slides the sidebar's content in from the side the project lies on, as Zen
 * switches spaces: a later project enters from the right, an earlier one from
 * the left. The elements move on the compositor, so the list never re-renders
 * for it.
 */
export function useProjectSpaceSlide(
  cwd: string,
  spaces: readonly string[],
  targets: readonly RefObject<HTMLElement | null>[],
  enabled: boolean,
) {
  const previous = useRef({ cwd, spaces });
  useLayoutEffect(() => {
    const before = previous.current;
    previous.current = { cwd, spaces };
    if (!enabled || sameProjectPath(before.cwd, cwd)) return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    const from = before.spaces.findIndex((path) =>
      sameProjectPath(path, before.cwd),
    );
    const to = spaces.findIndex((path) => sameProjectPath(path, cwd));
    if (from < 0 || to < 0) return;
    const direction = to > from ? 1 : -1;
    for (const target of targets) {
      const el = target.current;
      if (!el || typeof el.animate !== "function") continue;
      el.animate(
        [
          {
            transform: `translateX(${direction * SLIDE_DISTANCE}px)`,
            opacity: 0,
          },
          { transform: "none", opacity: 1 },
        ],
        { duration: SLIDE_MS, easing: SLIDE_EASING },
      );
    }
    // Only a project switch slides; the refs are stable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd]);
}

/** The inset sidebar's foot: settings, a dot per project, and a new one. */
export function ProjectSpacesBar({
  cwd,
  spaces,
  busyPaths,
  onSelectProject,
  onOpenProject,
  onOpenSettings,
}: {
  cwd: string;
  spaces: readonly string[];
  busyPaths?: Iterable<string>;
  onSelectProject: (path: string) => void;
  onOpenProject?: () => void;
  onOpenSettings?: () => void;
}) {
  const { groupLabels } = useProjectAppearance();
  const busy = useMemo(() => [...(busyPaths ?? [])], [busyPaths]);
  return (
    <div
      className="grid h-10 shrink-0 grid-cols-[auto_1fr_auto] items-center gap-1 px-2"
      data-tauri-drag-region="deep"
    >
      <div className="flex w-7 justify-start">
        {onOpenSettings ? (
          <IconButton label={`Settings (${MOD},)`} onClick={onOpenSettings}>
            <Settings className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
      </div>
      <nav
        aria-label="Projects"
        className="flex min-w-0 items-center justify-center gap-0.5 overflow-hidden"
      >
        {spaces.map((path) => {
          const current = sameProjectPath(path, cwd);
          const working = busy.some((busyPath) =>
            sameProjectPath(busyPath, path),
          );
          const label = resolveTabGroupLabel(
            projectKey(path),
            groupLabels,
            basename(path) || projectName(path),
          );
          return (
            <button
              key={path}
              type="button"
              title={label}
              aria-label={working ? `${label}, working` : label}
              aria-current={current ? "true" : undefined}
              data-tauri-drag-region="false"
              onClick={() => {
                if (!current) onSelectProject(path);
              }}
              className="group grid size-5 shrink-0 place-items-center rounded-full"
            >
              <span
                className={`block rounded-full transition-[width,height,background-color] duration-200 ease-out motion-reduce:transition-none ${
                  current
                    ? "size-2 bg-content/70"
                    : working
                      ? "size-1.5 bg-accent"
                      : "size-1.5 bg-content/25 group-hover:bg-content/55"
                }`}
              />
            </button>
          );
        })}
      </nav>
      <div className="flex w-7 justify-end">
        {onOpenProject ? (
          <IconButton label={`Open project (${MOD}O)`} onClick={onOpenProject}>
            <Plus className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
      </div>
    </div>
  );
}
