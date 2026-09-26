import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useT } from "../../../shared/hooks/useI18n";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import { basename } from "../../../platform/tauri/fs";
import { prettyParent, projectKey, projectName } from "../../../shared/lib/paths";
import {
  looksLikeProject,
  projectRailItems,
  sameProjectPath,
  subscribeProjectPathsChanged,
  type RecentProject,
} from "../model/recents";
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
import { Check, ChevronDown, Plus, Search } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { ProjectMascot } from "./ProjectMascot";

function loadAppearance() {
  return {
    groupLabels: loadTabGroupLabels(),
    groupColors: loadTabGroupColors(),
    groupCustomColors: loadTabGroupCustomColors(),
    groupMascots: loadTabGroupMascots(),
  };
}

type Props = {
  cwd: string;
  recents: RecentProject[];
  /** Project whose rail supplies the choices when it differs from `cwd`. */
  railCwd?: string;
  busy?: boolean;
  mode?: "switch" | "move";
  className?: string;
  buttonClassName?: string;
  appearance?: "ghost" | "filled";
  compact?: boolean;
  onSelectProject: (path: string) => void;
  onOpenProject?: () => void;
  /** Opens the project's context menu; the search input receives focus back. */
  onProjectContextMenu?: (
    path: string,
    x: number,
    y: number,
    trigger: HTMLElement | null,
  ) => void;
  /** Keeps the dropdown open while the project's context menu or its dialogs show. */
  projectMenuActive?: boolean;
};

export function SearchableProjectPicker({
  cwd,
  recents,
  railCwd,
  busy = false,
  mode = "switch",
  className,
  buttonClassName,
  appearance = "ghost",
  compact = false,
  onSelectProject,
  onOpenProject,
  onProjectContextMenu,
  projectMenuActive = false,
}: Props) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const pickerRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [{ groupLabels, groupColors, groupCustomColors, groupMascots }, setAppearance] =
    useState(loadAppearance);
  useEffect(
    () => subscribeProjectPathsChanged(() => setAppearance(loadAppearance())),
    [],
  );
  const groupLogos = useTabGroupLogos();
  const inProject = looksLikeProject(cwd);
  const seed = projectName(cwd);
  const key = projectKey(cwd);
  const label = inProject
    ? resolveTabGroupLabel(key, groupLabels, basename(cwd) || seed)
    : t("Choose project");
  const logoPath = resolveTabGroupLogo(key, groupLogos);
  const color = resolveTabGroupColor(key, groupColors, groupCustomColors, seed);
  const railProjects = projectRailItems(recents, railCwd ?? cwd);
  const projects =
    inProject && !railProjects.some((item) => sameProjectPath(item.path, cwd))
      ? [{ path: cwd, openedAt: 0 }, ...railProjects]
      : railProjects;
  const orderedProjects = [
    ...projects.filter((item) => sameProjectPath(item.path, cwd)),
    ...projects.filter((item) => !sameProjectPath(item.path, cwd)),
  ];
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredProjects = normalizedQuery
    ? orderedProjects.filter((item) => {
        const itemKey = projectKey(item.path);
        const itemLabel = resolveTabGroupLabel(
          itemKey,
          groupLabels,
          basename(item.path) || projectName(item.path),
        );
        return `${itemLabel}\n${item.path}`
          .toLocaleLowerCase()
          .includes(normalizedQuery);
      })
    : orderedProjects;

  const closePicker = () => {
    setOpen(false);
    setQuery("");
    setActive(0);
  };

  const openPicker = () => {
    setOpen(true);
    setQuery("");
    setActive(0);
  };

  useEffect(() => {
    if (!open) return;
    searchRef.current?.focus();
    const frame = window.requestAnimationFrame(() => {
      searchRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [open]);

  const pickProject = (path: string) => {
    closePicker();
    if (!sameProjectPath(path, cwd)) onSelectProject(path);
  };

  const onPickerKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    const target = event.target;
    const rows = listRef.current?.children;
    if (
      onProjectContextMenu &&
      rows &&
      (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10"))
    ) {
      // A Tab-focused row opens its own menu; the search input opens the highlighted one.
      const index =
        target instanceof HTMLInputElement
          ? active
          : Array.prototype.indexOf.call(rows, target);
      const project = filteredProjects[index];
      const row = rows[index];
      if (!project || !row) return;
      event.preventDefault();
      const rect = row.getBoundingClientRect();
      onProjectContextMenu(project.path, rect.left, rect.bottom, searchRef.current);
      return;
    }
    if (!(target instanceof HTMLInputElement)) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (filteredProjects.length === 0) return;
      setActive((index) => Math.min(filteredProjects.length - 1, index + 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, index - 1));
      return;
    }
    if (event.key === "Enter") {
      const project = filteredProjects[active];
      if (!project) return;
      event.preventDefault();
      pickProject(project.path);
    }
  };

  const action =
    mode === "move" ? t("Move note to project") : t("Switch project");

  return (
    <div
      ref={pickerRef}
      className={`relative flex h-full min-w-0 items-center${
        className ? ` ${className}` : ""
      }`}
    >
      <button
        type="button"
        title={inProject ? cwd : undefined}
        aria-label={
          inProject
            ? t("{action}, current project {project}", {
                action,
                project: label,
              })
            : t("Choose project for note")
        }
        aria-expanded={open}
        aria-haspopup="dialog"
        data-tauri-drag-region="false"
        onClick={() => (open ? closePicker() : openPicker())}
        onKeyDown={(event) => {
          if (open) return;
          if (event.key !== "ArrowDown") return;
          event.preventDefault();
          openPicker();
        }}
        className={`flex min-w-0 items-center rounded-md text-[12px] leading-none ${
          compact ? "size-8 justify-center p-0" : "h-6.5 gap-1.5 px-2"
        } ${
          open
            ? "bg-selection text-content"
            : appearance === "filled"
              ? "bg-content/10 text-content hover:bg-content/[0.14]"
              : "text-content/50 hover:bg-content/5 hover:text-content"
        }${buttonClassName ? ` ${buttonClassName}` : ""}`}
      >
        {!inProject ? null : logoPath ? (
          <ProjectLogoIcon
            path={logoPath}
            className={`${compact ? "size-4" : "size-3.5"} shrink-0 rounded-sm`}
            imageClassName={compact ? "size-4" : "size-3.5"}
          />
        ) : (
          <ProjectMascot
            project={seed}
            color={color}
            name={resolveTabGroupMascot(key, groupMascots)}
            className={`${compact ? "size-4" : "size-3"} shrink-0`}
            active={busy}
          />
        )}
        {compact ? null : (
          <>
            <span className="min-w-0 truncate font-medium text-content/90">
              {label}
            </span>
            <ChevronDown
              className={`size-3 shrink-0 text-content/45 transition-transform ${
                open ? "rotate-180" : ""
              }`}
              strokeWidth={1.75}
            />
          </>
        )}
      </button>
      {open ? (
        <Popover
          anchor={pickerRef}
          side="bottom"
          align="start"
          gap={4}
          width={286}
          maxHeight={380}
          role="dialog"
          aria-label={t("Project picker")}
          onDismiss={projectMenuActive ? undefined : closePicker}
          onKeyDown={onPickerKeyDown}
          className="flex flex-col overflow-hidden"
        >
          <label className="flex h-11 shrink-0 items-center gap-2.5 border-b border-stroke px-3 text-content/45 focus-within:text-content/70">
            <Search className="size-4 shrink-0" strokeWidth={1.75} />
            <span className="sr-only">{t("Search projects")}</span>
            <input
              ref={searchRef}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActive(0);
              }}
              placeholder={t("Search projects...")}
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35"
            />
          </label>
          <div
            ref={listRef}
            className="min-h-0 flex-1 overflow-y-auto overscroll-none p-1.5"
          >
            {filteredProjects.length > 0 ? (
              filteredProjects.map((item, index) => {
                const current = sameProjectPath(item.path, cwd);
                const itemKey = projectKey(item.path);
                const itemSeed = projectName(item.path);
                const itemLabel = resolveTabGroupLabel(
                  itemKey,
                  groupLabels,
                  basename(item.path) || itemSeed,
                );
                const itemLogo = resolveTabGroupLogo(itemKey, groupLogos);
                const itemColor = resolveTabGroupColor(
                  itemKey,
                  groupColors,
                  groupCustomColors,
                  itemSeed,
                );
                return (
                  <button
                    key={item.path}
                    type="button"
                    title={item.path}
                    onMouseEnter={() => setActive(index)}
                    onClick={() => pickProject(item.path)}
                    onContextMenu={
                      onProjectContextMenu
                        ? (event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            onProjectContextMenu(
                              item.path,
                              event.clientX,
                              event.clientY,
                              searchRef.current,
                            );
                          }
                        : undefined
                    }
                    className={`flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left ${
                      active === index
                        ? "bg-selection text-content"
                        : "text-content/75 hover:bg-content/5 hover:text-content"
                    }`}
                  >
                    <span className="grid size-4 shrink-0 place-items-center">
                      {current ? (
                        <Check className="size-3.5" strokeWidth={2} />
                      ) : itemLogo ? (
                        <ProjectLogoIcon
                          path={itemLogo}
                          className="size-4 rounded-sm"
                          imageClassName="size-4"
                        />
                      ) : (
                        <ProjectMascot
                          project={itemSeed}
                          color={itemColor}
                          name={resolveTabGroupMascot(itemKey, groupMascots)}
                          className="size-3.5"
                        />
                      )}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[13px] font-medium">
                      {itemLabel}
                    </span>
                    <span className="max-w-44 shrink truncate font-mono text-[11px] text-content/40">
                      {prettyParent(item.path)}
                    </span>
                  </button>
                );
              })
            ) : (
              <p className="px-2.5 py-5 text-center text-[12px] text-content/45">
                {t("No projects found")}
              </p>
            )}
          </div>
          {onOpenProject ? (
            <div className="shrink-0 border-t border-stroke p-1.5">
              <button
                type="button"
                onClick={() => {
                  closePicker();
                  onOpenProject();
                }}
                className="flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] text-content/75 hover:bg-content/8 hover:text-content"
              >
                <Plus className="size-4 shrink-0" strokeWidth={1.75} />
                <span>{t("New project")}</span>
              </button>
            </div>
          ) : null}
        </Popover>
      ) : null}
    </div>
  );
}
