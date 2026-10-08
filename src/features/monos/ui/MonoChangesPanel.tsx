import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { lazySurface } from "../../../shared/ui/lazySurface";
import { ChevronDown, Folder } from "../../../shared/ui/icons";
import {
  gitLocateFiles,
  subscribeGitChanged,
  type GitDiffIndex,
} from "../../../platform/tauri/fs";
import { projectName } from "../../../shared/lib/paths";
import { BranchPicker } from "../../source-control/ui/BranchPicker";
import {
  sessionCheckpointStatus,
  subscribeReviewChanged,
} from "../../sessions/model/checkpoint";
import type { HarnessId } from "../../sessions/model/session";
import { MonoSidebar, MonoSidebarHeader } from "./MonoSidebar";
import {
  MonoProjectCommit,
  useProjectIndex,
  type MonoProjectFile,
} from "./MonoProjectCommit";

const SessionChangesDiff = lazySurface(async () => {
  const module = await import("../../source-control/ui/SessionChangesDiff");
  return { default: module.SessionChangesDiff };
});

export type MonoChangesTab = "changes" | "commit";

/** What the chat asked the panel to show; a new object re-applies it. */
export type MonoChangesRequest = { path?: string; tab: MonoChangesTab };

/**
 * A Mono's session changes beside the chat, per project they landed in:
 * review the diff on one tab, commit and push it on the other.
 */
export function MonoChangesPanel({
  sessionId,
  cwd,
  request,
  color,
  textHarness,
  onClose,
  windowControls,
}: {
  sessionId: string;
  cwd: string;
  request: MonoChangesRequest;
  color: string;
  textHarness?: HarnessId;
  onClose: () => void;
  windowControls?: ReactNode;
}) {
  const { projects, outside, loaded } = useSessionProjects(sessionId, cwd);
  const [tab, setTab] = useState<MonoChangesTab>(request.tab);
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
  const { index, reload } = useProjectIndex(project?.root);
  // One lock for the header's branch picker and the Commit tab's actions, so
  // a checkout never overlaps a commit, push or discard in the same panel.
  const [busy, setBusy] = useState<string | null>(null);
  const onCheckoutBusy = useCallback((working: boolean) => {
    setBusy((current) =>
      working ? "checkout" : current === "checkout" ? null : current,
    );
  }, []);

  useEffect(() => {
    setTab(request.tab);
    setFocusPath(request.path);
    if (request.path) setSelectedRoot(null);
  }, [request]);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [onClose]);

  // With one repository the diff shows everything; with several, the
  // selected project's files plus anything outside Git stay together.
  const scope =
    projects.length > 1 && project
      ? [...project.files.map((entry) => entry.file.path), ...outside]
      : undefined;

  return (
    <MonoSidebar
      open
      kind="changes"
      label="Changes"
      color={color}
      windowControls={windowControls}
    >
      <MonoSidebarHeader
        title="Changes"
        heading={
          project ? (
            <ProjectHeading
              root={project.root}
              index={index}
              projects={projects.map((entry) => entry.root)}
              onSelect={setSelectedRoot}
              branchEnabled={busy === null || busy === "checkout"}
              onCheckoutBusy={onCheckoutBusy}
            />
          ) : undefined
        }
        onClose={onClose}
      />
      <div
        role="tablist"
        aria-label="Changes views"
        className="flex h-9 shrink-0 items-stretch gap-4 border-b border-stroke px-4"
      >
        <TabButton
          selected={tab === "changes"}
          onClick={() => setTab("changes")}
        >
          Changes
          {project && project.files.length > 0 ? (
            <span className="tabular-nums text-content/40">
              {project.files.length}
            </span>
          ) : null}
        </TabButton>
        <TabButton
          selected={tab === "commit"}
          disabled={!project}
          onClick={() => setTab("commit")}
        >
          Commit
        </TabButton>
      </div>
      <div className="relative flex min-h-0 flex-1 flex-col">
        {/* Both tabs stay mounted so switching back is instant and the
            commit draft survives a look at the diff. */}
        <div
          role="tabpanel"
          aria-label="Changes"
          hidden={tab !== "changes" && !!project}
          className="absolute inset-0 h-full"
        >
          <SessionChangesDiff
            cwd={cwd}
            sessionId={sessionId}
            focusPath={focusPath}
            paths={scope}
          />
        </div>
        {project ? (
          <div
            role="tabpanel"
            aria-label="Commit"
            hidden={tab !== "commit"}
            className="absolute inset-0 flex h-full flex-col"
          >
            <MonoProjectCommit
              key={project.root}
              sessionId={sessionId}
              cwd={cwd}
              root={project.root}
              files={project.files}
              index={index}
              reloadIndex={reload}
              busy={busy}
              setBusy={setBusy}
              textHarness={textHarness}
              onOpenFile={(path) => {
                setFocusPath(path);
                setTab("changes");
              }}
            />
          </div>
        ) : null}
      </div>
      {loaded && !project && outside.length > 0 ? (
        <p className="shrink-0 border-t border-stroke px-4 py-2 text-[11px] text-content/45">
          These files aren’t in a Git repository, so there’s nothing to commit.
        </p>
      ) : null}
    </MonoSidebar>
  );
}

function TabButton({
  selected,
  disabled,
  onClick,
  children,
}: {
  selected: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      disabled={disabled}
      onClick={onClick}
      className={`-mb-px flex items-center gap-1.5 border-b text-[12px] font-medium disabled:opacity-35 ${
        selected
          ? "border-content text-content"
          : "border-transparent text-content/50 hover:text-content/80"
      }`}
    >
      {children}
    </button>
  );
}

/** The project the work landed in, switchable when a Mono touched several. */
function ProjectHeading({
  root,
  index,
  projects,
  onSelect,
  branchEnabled,
  onCheckoutBusy,
}: {
  root: string;
  index: GitDiffIndex | null;
  projects: string[];
  onSelect: (root: string) => void;
  branchEnabled: boolean;
  onCheckoutBusy: (busy: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const switchable = projects.length > 1;

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, [open]);

  const name = (
    <>
      <Folder
        className="size-3.5 shrink-0 text-content/50"
        strokeWidth={1.75}
      />
      <span className="min-w-0 truncate">{projectName(root)}</span>
      {switchable ? (
        <ChevronDown
          className="size-3 shrink-0 text-content/45"
          strokeWidth={2}
        />
      ) : null}
    </>
  );

  return (
    <div className="flex min-w-0 items-center gap-2">
      <div
        ref={menuRef}
        className="relative min-w-0"
        data-tauri-drag-region="false"
      >
        {switchable ? (
          <button
            type="button"
            title={root}
            aria-haspopup="menu"
            aria-expanded={open}
            onClick={() => setOpen((value) => !value)}
            className="-ml-1.5 flex min-w-0 items-center gap-1.5 rounded-md px-1.5 py-0.5 hover:bg-content/8 aria-expanded:bg-content/8"
          >
            {name}
          </button>
        ) : (
          <span title={root} className="flex min-w-0 items-center gap-1.5">
            {name}
          </span>
        )}
        {open ? (
          <div
            role="menu"
            aria-label="Projects"
            className="absolute top-full left-0 z-30 mt-1 min-w-44 rounded-md border border-content/10 bg-background-base py-1 shadow-lg"
          >
            {projects.map((entry) => (
              <button
                key={entry}
                type="button"
                role="menuitemradio"
                aria-checked={entry === root}
                title={entry}
                onClick={() => {
                  onSelect(entry);
                  setOpen(false);
                }}
                className="flex h-7 w-full items-center gap-2 px-3 text-left text-[12px] font-normal text-content hover:bg-content/10 aria-checked:font-medium"
              >
                <Folder className="size-3.5 shrink-0" strokeWidth={1.75} />
                <span className="min-w-0 truncate">{projectName(entry)}</span>
              </button>
            ))}
          </div>
        ) : null}
      </div>
      {index?.branch ? (
        <span className="flex min-w-0 items-center gap-1 text-[11px] font-normal text-content/50">
          <BranchPicker
            cwd={root}
            branch={index.branch}
            enabled={branchEnabled}
            onBusyChange={onCheckoutBusy}
            popoverSide="bottom"
            compact
          />
          {index.ahead > 0 ? (
            <span className="shrink-0 tabular-nums text-content/40">
              ↑{index.ahead}
            </span>
          ) : null}
          {index.behind > 0 ? (
            <span className="shrink-0 tabular-nums text-content/40">
              ↓{index.behind}
            </span>
          ) : null}
        </span>
      ) : null}
    </div>
  );
}

type SessionProject = { root: string; files: MonoProjectFile[] };

/**
 * Group the session's changed files by the repository that holds them. A
 * project stays listed after its files are committed so it can still be pushed.
 */
function useSessionProjects(
  sessionId: string,
  cwd: string,
): { projects: SessionProject[]; outside: string[]; loaded: boolean } {
  const [projects, setProjects] = useState<SessionProject[]>([]);
  const [outside, setOutside] = useState<string[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    setProjects([]);
    setOutside([]);
    setLoaded(false);
    if (!cwd || cwd === "~") return;
    let disposed = false;
    let generation = 0;
    const load = () => {
      const current = ++generation;
      void sessionCheckpointStatus(sessionId, cwd)
        .then(async ({ files }) => {
          const locations = await gitLocateFiles(
            files.map((file) => file.path),
          );
          if (disposed || current !== generation) return;
          const byRoot = new Map<string, MonoProjectFile[]>();
          const loose: string[] = [];
          files.forEach((file, index) => {
            const location = locations[index];
            if (!location) {
              loose.push(file.path);
              return;
            }
            const group = byRoot.get(location.root) ?? [];
            group.push({ file, relative: location.relative });
            byRoot.set(location.root, group);
          });
          setOutside(loose);
          setProjects((previous) => {
            const next = previous.map((project) => ({
              root: project.root,
              files: byRoot.get(project.root) ?? [],
            }));
            for (const [root, group] of byRoot) {
              if (!previous.some((project) => project.root === root)) {
                next.push({ root, files: group });
              }
            }
            return next;
          });
        })
        .catch(() => undefined)
        .finally(() => {
          if (!disposed && current === generation) setLoaded(true);
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
  }, [sessionId, cwd]);

  return { projects, outside, loaded };
}
