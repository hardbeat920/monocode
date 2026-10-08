import { useRef, useState } from "react";
import { Plus, X } from "../../../shared/ui/icons";
import { projectKey, projectName } from "../../../shared/lib/paths";
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
      className="size-3 shrink-0"
    />
  );
}

/**
 * The projects a Mono works on, under its model in the details panel. Each
 * can be taken away; more are added from the projects on the rail.
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
    <div className="flex min-w-0 flex-1 flex-col" data-mono-projects>
      {projects.map((project) => (
        <div
          key={project.path}
          title={project.path}
          className="group flex h-7 min-w-0 items-center gap-2 text-content/85"
        >
          <ProjectFace path={project.path} />
          <span className="min-w-0 flex-1 truncate">{project.name}</span>
          <button
            type="button"
            aria-label={`Remove ${project.name}`}
            title={`Remove ${project.name}`}
            onClick={() => removeMonoProject(monoId, project.path)}
            className="grid size-5 shrink-0 place-items-center rounded text-content/45 opacity-0 hover:bg-content/8 hover:text-content focus-visible:opacity-100 group-hover:opacity-100"
          >
            <X className="size-3" strokeWidth={1.75} />
          </button>
        </div>
      ))}
      <button
        ref={anchor}
        type="button"
        aria-haspopup="dialog"
        aria-expanded={adding}
        onClick={() => setAdding((open) => !open)}
        className="-ml-1 flex h-7 w-fit items-center gap-1.5 rounded-md px-1 text-content/45 hover:bg-content/6 hover:text-content aria-expanded:bg-content/6 aria-expanded:text-content"
      >
        <Plus className="size-3 shrink-0" strokeWidth={1.75} />
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
