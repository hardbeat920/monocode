import { useRef, useState } from "react";
import { Plus, X } from "../../../shared/ui/icons";
import { prettyCwd, projectKey, projectName } from "../../../shared/lib/paths";
import { loadRecents, projectRailItems } from "../../projects/model/recents";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import { ProjectPickerPopover } from "../../projects/ui/SearchableProjectPicker";
import {
  addMonoProject,
  removeMonoProject,
  type MonoProject,
} from "../model/mono";

/** A project as the rail shows it: its mascot in its color, then its name. */
function ProjectFace({ path }: { path: string }) {
  const key = projectKey(path);
  const seed = projectName(path);
  return (
    <ProjectMascot
      project={seed}
      color={resolveTabGroupColor(
        key,
        loadTabGroupColors(),
        loadTabGroupCustomColors(),
        seed,
      )}
      name={resolveTabGroupMascot(key, loadTabGroupMascots())}
      className="size-4 shrink-0"
    />
  );
}

/**
 * The projects a Mono works on, one row each in its settings: face, name and
 * folder. Each can be taken away; more are added from the projects on the rail.
 */
export function MonoProjects({
  monoId,
  projects,
}: {
  monoId: string;
  projects: readonly MonoProject[];
}) {
  const anchor = useRef<HTMLButtonElement>(null);
  const [adding, setAdding] = useState(false);
  const taken = new Set(projects.map((project) => projectKey(project.path)));
  const choices = adding
    ? projectRailItems(loadRecents(), "").filter(
        (item) => !taken.has(projectKey(item.path)),
      )
    : [];
  return (
    <div
      className="flex min-w-0 flex-col divide-y divide-stroke"
      data-mono-projects
    >
      {projects.map((project) => (
        <div
          key={project.path}
          className="group flex min-h-12 min-w-0 items-center gap-3 px-3 py-2"
        >
          <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-content/5">
            <ProjectFace path={project.path} />
          </span>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-[13px] leading-5 text-content/90">
              {project.name}
            </span>
            <span
              className="truncate text-[11px] leading-4 text-content/40"
              title={project.path}
            >
              {prettyCwd(project.path)}
            </span>
          </span>
          <button
            type="button"
            aria-label={`Remove ${project.name}`}
            title={`Remove ${project.name}`}
            onClick={() => removeMonoProject(monoId, project.path)}
            className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 opacity-0 hover:bg-content/8 hover:text-content focus-visible:opacity-100 group-hover:opacity-100"
          >
            <X className="size-3.5" strokeWidth={1.75} />
          </button>
        </div>
      ))}
      <button
        ref={anchor}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={adding}
        onClick={() => setAdding((open) => !open)}
        className="flex h-11 w-full items-center gap-3 px-3 text-left text-[13px] text-content/55 first:rounded-t-xl last:rounded-b-xl hover:bg-content/4 hover:text-content aria-expanded:bg-content/4 aria-expanded:text-content"
      >
        <span className="grid size-8 shrink-0 place-items-center rounded-lg border border-dashed border-content/15">
          <Plus className="size-3.5 shrink-0" strokeWidth={1.75} />
        </span>
        Add project
      </button>
      {adding ? (
        <ProjectPickerPopover
          anchor={anchor}
          projects={choices}
          onDismiss={() => setAdding(false)}
          onSelectProject={(path) => addMonoProject(monoId, path)}
          label="Add a project"
          emptyMessage={
            choices.length === 0
              ? "It already works on every project."
              : undefined
          }
        />
      ) : null}
    </div>
  );
}
