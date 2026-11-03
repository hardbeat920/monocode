import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ComponentProps,
} from "react";
import { Popover } from "../../../shared/ui/Popover";
import {
  Check,
  ChevronDown,
  FileDiff,
  FolderTree,
  GitBranch,
} from "../../../shared/ui/icons";
import {
  basename,
  gitLocateFiles,
  subscribeGitChanged,
  type GitDiffIndex,
} from "../../../platform/tauri/fs";
import { prettyCwd, projectKey, projectName } from "../../../shared/lib/paths";
import {
  sessionCheckpointStatus,
  subscribeReviewChanged,
} from "../../sessions/model/checkpoint";
import type { HarnessId } from "../../sessions/model/session";
import { useProjectAppearance } from "../../projects/hooks/useProjectAppearance";
import { useTabGroupLogos } from "../../projects/hooks/useTabGroupLogos";
import { ProjectLogoIcon } from "../../projects/ui/ProjectLogoIcon";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import {
  resolveTabGroupColor,
  resolveTabGroupLabel,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";
import { EmptyState, PageHeader } from "./monoPanelParts";
import { listWorktrees } from "../../source-control/model/worktrees";
import {
  MonoProjectCommit,
  useProjectIndex,
  type MonoProjectFile,
} from "./MonoProjectCommit";
import { lazySurface } from "../../../shared/ui/lazySurface";
import { isImagePath, isVideoPath } from "../../files/model/filePreview";
import { BinaryFileView } from "../../files/ui/BinaryFileView";

const FileEditor = lazySurface(async () => {
  const module = await import("../../files/ui/FileEditor");
  return { default: module.FileEditor };
});

/** What the chat asked the view to show; a new object re-applies it. */
export type MonoChangesRequest = { path?: string };

/**
 * A Mono's session changes, one tab per project they landed in, each with
 * its commit and push.
 */
export function MonoChangesView({
  sessionId,
  cwd,
  request,
  textHarness,
  agentName,
  attachedProjects,
}: {
  sessionId: string;
  cwd: string;
  request: MonoChangesRequest;
  textHarness?: HarnessId;
  /** The Mono, named in the empty state. */
  agentName: string;
  /** Projects given to the Mono; each has a tab even before it has changes. */
  attachedProjects: readonly string[];
}) {
  const { projects, outside, loaded, error } = useSessionProjects(
    sessionId,
    cwd,
    attachedProjects,
  );
  const [focusPath, setFocusPath] = useState(request.path);
  const [selectedRoot, setSelectedRoot] = useState<string | null>(null);
  const project =
    projects.find((entry) => entry.root === selectedRoot) ??
    (focusPath
      ? projects.find((entry) =>
          entry.files.some((item) => item.file.path === focusPath),
        )
      : undefined) ??
    projects[0];
  const [indexes, setIndexes] = useState<ReadonlyMap<string, GitDiffIndex | null>>(new Map());
  const updateIndex = useCallback((root: string, index: GitDiffIndex | null) => {
    setIndexes((previous) => previous.get(root) === index ? previous : new Map(previous).set(root, index));
  }, []);

  // A changed file, open with its changes over its checkout's list until Back.
  const [diff, setDiff] = useState<{ root: string; path: string } | null>(null);

  useEffect(() => {
    setFocusPath(request.path);
    setDiff(null);
    if (request.path) setSelectedRoot(null);
  }, [request]);
  useEffect(() => setDiff(null), [sessionId, cwd]);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-mono-changes>
      {diff ? (
        <>
          <PageHeader
            title={basename(diff.path)}
            onBack={() => setDiff(null)}
          />
          <div className="relative min-h-0 flex-1">
            <div className="absolute inset-0">
              {isImagePath(diff.path) || isVideoPath(diff.path) ? (
                <BinaryFileView path={diff.path} cwd={diff.root} visible />
              ) : (
                <FileEditor
                  key={diff.path}
                  path={diff.path}
                  cwd={diff.root}
                  showDiff
                  active
                  onDirtyChange={() => {}}
                />
              )}
            </div>
          </div>
        </>
      ) : null}
      {project && !diff ? (
        <ProjectTabs
          projects={projects}
          selected={project}
          indexes={indexes}
          onSelect={setSelectedRoot}
        />
      ) : null}
      {project ? (
        <div className="relative min-h-0 flex-1" hidden={!!diff}>
          {/* Every checkout stays mounted so its draft survives a switch. */}
          {projects.map((entry) => (
            <div
              key={`${sessionId}:${cwd}:${entry.root}`}
              role="tabpanel"
              aria-label={`Commit ${projectName(entry.root)}`}
              hidden={entry.root !== project.root}
              className="absolute inset-0 flex h-full flex-col"
            >
              <ProjectCommit
                sessionId={sessionId}
                cwd={cwd}
                root={entry.root}
                files={entry.files}
                onIndex={updateIndex}
                textHarness={textHarness}
                onOpenFile={(path) => setDiff({ root: entry.root, path })}
              />
            </div>
          ))}
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          {/* With no repository at all there's nothing to commit to. */}
          {loaded && !error ? (
            <EmptyState icon={FileDiff}>
              {outside.length > 0
                ? "These files aren’t in a Git repository, so there’s nothing to commit."
                : `Give ${agentName} a project to work on, and its changes show up here to commit.`}
            </EmptyState>
          ) : null}
        </div>
      )}
      {error ? (
        <p role="alert" className="shrink-0 border-t border-stroke px-4 py-2 text-[12px] text-red-400">
          Couldn’t load repositories: {error}
        </p>
      ) : null}
    </div>
  );
}

/** Each checkout stays mounted so its draft, selection and actions survive a switch. */
function ProjectCommit({
  onIndex,
  ...props
}: Omit<ComponentProps<typeof MonoProjectCommit>, "index" | "reloadIndex"> & {
  onIndex: (root: string, index: GitDiffIndex | null) => void;
}) {
  const { index, reload } = useProjectIndex(props.root);
  useEffect(() => onIndex(props.root, index), [props.root, index, onIndex]);
  return <MonoProjectCommit {...props} index={index} reloadIndex={reload} />;
}

/**
 * Every project the work landed in, as a tab with its logo or mascot. A
 * project whose changes span several checkouts gets a chevron that switches
 * between them.
 */
function ProjectTabs({
  projects,
  selected,
  indexes,
  onSelect,
}: {
  projects: SessionProject[];
  selected: SessionProject;
  indexes: ReadonlyMap<string, GitDiffIndex | null>;
  onSelect: (root: string) => void;
}) {
  const { groupLabels, groupColors, groupCustomColors, groupMascots } =
    useProjectAppearance();
  const groupLogos = useTabGroupLogos();
  // The checkout last opened in each project, so its tab returns there.
  const [lastRoots, setLastRoots] = useState<ReadonlyMap<string, string>>(
    new Map(),
  );
  useEffect(() => {
    setLastRoots((previous) =>
      previous.get(selected.main) === selected.root
        ? previous
        : new Map(previous).set(selected.main, selected.root),
    );
  }, [selected.main, selected.root]);
  const groups = new Map<string, SessionProject[]>();
  for (const entry of projects) {
    groups.set(entry.main, [...(groups.get(entry.main) ?? []), entry]);
  }
  return (
    <div
      role="tablist"
      aria-label="Projects"
      // Sideways only, with the native bounce at either end.
      className="flex h-9 shrink-0 items-center gap-px overflow-x-auto overflow-y-hidden overscroll-x-contain border-b border-stroke px-2"
    >
      {[...groups].map(([main, checkouts]) => {
        const key = projectKey(main);
        const seed = projectName(main);
        const label = resolveTabGroupLabel(key, groupLabels, seed);
        const logoPath = resolveTabGroupLogo(key, groupLogos);
        const active = main === selected.main;
        const count = checkouts.reduce(
          (total, entry) => total + entry.files.length,
          0,
        );
        const target =
          checkouts.find(
            (entry) => entry.root === (active ? selected.root : lastRoots.get(main)),
          ) ?? checkouts[0];
        return (
          <div
            key={main}
            className={`flex h-6 shrink-0 items-center gap-0.5 rounded-md text-[12px] leading-none ${
              active
                ? "bg-selection text-content"
                : "text-content/50 hover:text-content/80"
            } ${checkouts.length > 1 ? "pr-1" : ""}`}
          >
            <button
              type="button"
              role="tab"
              aria-selected={active}
              title={target.root}
              data-mono-project={main}
              onClick={() => onSelect(target.root)}
              className={`flex h-full items-center gap-1.5 pl-2 ${
                checkouts.length > 1 ? "" : "pr-2"
              }`}
            >
              {logoPath ? (
                <ProjectLogoIcon
                  path={logoPath}
                  className="size-3.5 shrink-0 rounded-sm"
                  imageClassName="size-3.5"
                />
              ) : (
                <ProjectMascot
                  project={seed}
                  color={resolveTabGroupColor(
                    key,
                    groupColors,
                    groupCustomColors,
                    seed,
                  )}
                  name={resolveTabGroupMascot(key, groupMascots)}
                  className="size-3 shrink-0"
                />
              )}
              <span className="max-w-40 truncate">{label}</span>
              {count > 0 ? (
                <span className="tabular-nums text-content/40">{count}</span>
              ) : null}
            </button>
            {checkouts.length > 1 ? (
              <CheckoutPicker
                project={label}
                checkouts={checkouts}
                selected={active ? selected.root : undefined}
                indexes={indexes}
                onSelect={onSelect}
              />
            ) : null}
          </div>
        );
      })}
    </div>
  );
}

/** The checkouts of one project that hold changes, like the sidebar's working-copy switcher. */
function CheckoutPicker({
  project,
  checkouts,
  selected,
  indexes,
  onSelect,
}: {
  project: string;
  checkouts: SessionProject[];
  selected?: string;
  indexes: ReadonlyMap<string, GitDiffIndex | null>;
  onSelect: (root: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={anchor}
        type="button"
        aria-label={`Switch ${project} working copy`}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="grid size-4.5 place-items-center rounded text-content/45 hover:bg-content/8 hover:text-content aria-expanded:bg-content/8"
      >
        <ChevronDown className="size-3" />
      </button>
      {open ? (
        <Popover
          anchor={anchor}
          side="bottom"
          align="start"
          width={260}
          maxHeight={320}
          onDismiss={() => setOpen(false)}
          role="listbox"
          aria-label={`${project} working copies`}
          className="overflow-y-auto overscroll-none p-1.5"
        >
          {checkouts.map(({ root, main, files }) => {
            const branch = indexes.get(root)?.branch;
            return (
              <button
                key={root}
                type="button"
                role="option"
                aria-selected={root === selected}
                title={root}
                onClick={() => {
                  onSelect(root);
                  setOpen(false);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-content/5"
              >
                {root === main ? (
                  <GitBranch className="size-3.5 shrink-0 text-content/50" />
                ) : (
                  <FolderTree className="size-3.5 shrink-0 text-content/50" />
                )}
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12px]">
                    {branch ?? projectName(root)}
                  </span>
                  <span className="block truncate text-[10px] text-content/40">
                    {root === main ? "Project folder" : prettyCwd(root)}
                  </span>
                </span>
                {files.length > 0 ? (
                  <span className="shrink-0 text-[11px] tabular-nums text-content/40">
                    {files.length}
                  </span>
                ) : null}
                {root === selected ? <Check className="size-3.5 shrink-0" /> : null}
              </button>
            );
          })}
        </Popover>
      ) : null}
    </>
  );
}

/**
 * A checkout with changes, and the project folder it is a worktree of. A
 * project attached to the Mono is listed too, as a placeholder, while the
 * work hasn't touched it.
 */
type SessionProject = {
  root: string;
  main: string;
  files: MonoProjectFile[];
  placeholder?: boolean;
};

/** A path inside `folder`, so locating it finds the checkout `folder` is in. */
const folderProbe = (folder: string) =>
  `${folder.replace(/\/+$/, "")}/.monocode-probe`;

const mainCheckouts = new Map<string, Promise<string>>();

/** The project folder a checkout belongs to; a worktree resolves to its main checkout. */
function mainCheckout(root: string): Promise<string> {
  let main = mainCheckouts.get(root);
  if (!main) {
    main = listWorktrees(root).then(
      ({ worktrees }) => worktrees.find((tree) => tree.isMain)?.path ?? root,
      () => root,
    );
    mainCheckouts.set(root, main);
  }
  return main;
}

type SessionProjects = { projects: SessionProject[]; outside: string[] };

/** The last grouping per session, so switching back to a Mono opens on it. */
const sessionProjectsCache = new Map<string, SessionProjects>();

/**
 * Group the session's changed files by the repository that holds them. A
 * project stays listed after its files are committed so it can still be pushed.
 */
function useSessionProjects(
  sessionId: string,
  cwd: string,
  attachedProjects: readonly string[],
): SessionProjects & { loaded: boolean; error: string | null } {
  const attachedKey = attachedProjects.join("\n");
  const key = `${sessionId}\n${cwd}\n${attachedKey}`;
  const [state, setState] = useState<SessionProjects & { key: string }>(() => ({
    key,
    ...(sessionProjectsCache.get(key) ?? { projects: [], outside: [] }),
  }));
  const [loadedKey, setLoadedKey] = useState<string | null>(() =>
    sessionProjectsCache.has(key) ? key : null,
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const cached = sessionProjectsCache.get(key);
    setState({ key, ...(cached ?? { projects: [], outside: [] }) });
    setLoadedKey(cached ? key : null);
    setError(null);
    if (!cwd || cwd === "~") return;
    const attached = attachedKey ? attachedKey.split("\n") : [];
    let disposed = false;
    let generation = 0;
    const load = () => {
      const current = ++generation;
      void sessionCheckpointStatus(sessionId, cwd)
        .then(async ({ files }) => {
          const located = await gitLocateFiles([
            ...attached.map(folderProbe),
            ...files.map((file) => file.path),
          ]);
          const attachedLocations = located.slice(0, attached.length);
          const locations = located.slice(attached.length);
          const roots = [
            ...new Set(
              located.flatMap(
                (location) => location?.root ?? [],
              ),
            ),
          ];
          const mains = new Map(
            await Promise.all(
              roots.map(async (root) => [root, await mainCheckout(root)] as const),
            ),
          );
          if (disposed || current !== generation) return;
          setError(null);
          const byRoot = new Map<string, MonoProjectFile[]>();
          const outside: string[] = [];
          files.forEach((file, index) => {
            const location = locations[index];
            if (!location) {
              outside.push(file.path);
              return;
            }
            const group = byRoot.get(location.root) ?? [];
            group.push({ file, relative: location.relative });
            byRoot.set(location.root, group);
          });
          const previous = (
            sessionProjectsCache.get(key)?.projects ?? []
          ).filter((project) => !project.placeholder);
          const projects = previous.map((project) => ({
            ...project,
            files: byRoot.get(project.root) ?? [],
          }));
          for (const [root, group] of byRoot) {
            if (!previous.some((project) => project.root === root)) {
              projects.push({ root, main: mains.get(root) ?? root, files: group });
            }
          }
          for (const location of attachedLocations) {
            const root = location?.root;
            if (!root || projects.some((project) => project.root === root))
              continue;
            projects.push({
              root,
              main: mains.get(root) ?? root,
              files: [],
              placeholder: true,
            });
          }
          sessionProjectsCache.set(key, { projects, outside });
          setState({ key, projects, outside });
        })
        .catch((caught: unknown) => {
          if (!disposed && current === generation)
            setError(caught instanceof Error ? caught.message : String(caught));
        })
        .finally(() => {
          if (!disposed && current === generation) setLoadedKey(key);
        });
    };
    load();
    const unsubscribeReview = subscribeReviewChanged((changed) => {
      if (!changed || changed === sessionId) load();
    });
    const unsubscribeGit = subscribeGitChanged(load);
    return () => {
      disposed = true;
      unsubscribeReview();
      unsubscribeGit();
    };
  }, [key, sessionId, cwd, attachedKey]);

  // Before the effect catches up with a switch, show the new session's cache.
  const current =
    state.key === key
      ? state
      : (sessionProjectsCache.get(key) ?? { projects: [], outside: [] });
  return {
    projects: current.projects,
    outside: current.outside,
    loaded: loadedKey === key || sessionProjectsCache.has(key),
    error,
  };
}
