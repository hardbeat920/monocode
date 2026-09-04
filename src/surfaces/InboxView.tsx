import { openUrl } from "@tauri-apps/plugin-opener";
import {
  CheckCheck,
  ChevronDown,
  CircleDot,
  CircleX,
  ExternalLink,
  GitCompare,
  GitMerge,
  GitPullRequest,
  GitPullRequestClosed,
  GitPullRequestDraft,
  Inbox,
  ListFilter,
  LoaderCircle,
  RefreshCw,
  Search,
  type IconComponent,
} from "../chrome/icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import {
  InboxFiltersMenu,
  INBOX_FILTER_MENU_WIDTH,
} from "../chrome/InboxFiltersMenu";
import { InboxProviderMark } from "../chrome/InboxProviderMark";
import { ProjectLogoIcon } from "../chrome/ProjectLogoIcon";
import { ProjectMascot } from "../chrome/ProjectMascot";
import { OverlayNav } from "../chrome/TitleBar";
import { WindowControls } from "../chrome/WindowControls";
import { useDragResize } from "../hooks/useDragResize";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import {
  githubPrDiff,
  githubReviewDecisionLabel,
  githubWorkItemComment,
  githubWorkItemDetails,
  githubWorkItemThread,
  inboxItemKey,
  inboxItemRef,
  inboxItemStatus,
  inboxListIsFresh,
  inboxProjectsForRail,
  listInboxItems,
  peekGithubPrDiff,
  peekGithubWorkItemDetails,
  peekGithubWorkItemThread,
  peekInboxList,
  formatRelativeTime,
  inboxPersonAvatarUrl,
  type GithubLabel,
  type GithubPrDiff,
  type GithubWorkItemDetails,
  type GithubWorkItemThread,
  type InboxItem,
  type InboxProviderErrors,
  type InboxQuery,
} from "../lib/githubTasks";
import {
  applyInboxFilters,
  hasActiveInboxFilters,
  inboxFetchState,
  loadInboxFilters,
  loadInboxSource,
  pruneInboxFilters,
  saveInboxFilters,
  saveInboxSource,
  type InboxFilters,
  type InboxSource,
} from "../lib/inboxFilters";
import { projectName } from "../lib/paths";
import { IS_MAC } from "../lib/platform";
import { sameProjectPath, type RecentProject } from "../lib/recents";
import {
  isInboxEntryUnseen,
  markInboxItemSeen,
  markInboxItemsSeen,
  useInboxSeenTick,
} from "../lib/inboxSeen";
import {
  LINEAR_CHANGE_EVENT,
  linearIssueComment,
  linearIssueDetails,
  linearIssueThread,
  loadHiddenLinearTeamIds,
  peekLinearIssueDetails,
  peekLinearIssueThread,
  type LinearIssueThread,
} from "../lib/linear";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../lib/tabGroups";
import { AgentMarkdown } from "./AgentMarkdown";
import {
  InboxComments,
  InboxCommentForm,
  type InboxReplyTarget,
} from "./InboxComments";
import { InboxPrDiff } from "./InboxPrDiff";

const MIN_WIDTH = 240;
const MAX_WIDTH = 420;
const DEFAULT_WIDTH = 280;

let rememberedWidth = DEFAULT_WIDTH;

type InboxProjectOption = {
  path: string;
  name: string;
  logoPath: string | null;
  mascotName: string | null;
  mascotColor: string;
};

function inboxProjectOptions(
  projects: RecentProject[],
  logos: ReturnType<typeof useTabGroupLogos>,
): InboxProjectOption[] {
  const mascots = loadTabGroupMascots();
  const colors = loadTabGroupColors();
  const custom = loadTabGroupCustomColors();
  return [...projects]
    .map((project) => {
      const key = projectName(project.path);
      return {
        path: project.path,
        name: key,
        logoPath: resolveTabGroupLogo(key, logos),
        mascotName: resolveTabGroupMascot(key, mascots),
        mascotColor: resolveTabGroupColor(key, colors, custom, key),
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

function InboxProjectMark({
  project,
}: {
  project: Pick<
    InboxProjectOption,
    "name" | "logoPath" | "mascotName" | "mascotColor"
  >;
}) {
  if (project.logoPath) {
    return (
      <ProjectLogoIcon
        path={project.logoPath}
        className="size-3.5 shrink-0 rounded-sm"
        imageClassName="size-3.5"
      />
    );
  }
  return (
    <ProjectMascot
      project={project.name}
      color={project.mascotColor}
      name={project.mascotName}
      className="size-3 shrink-0"
    />
  );
}

function peekInboxForRail(recents: RecentProject[], cwd: string) {
  const projects = inboxProjectsForRail(recents, cwd);
  const filters = pruneInboxFilters(
    loadInboxFilters(),
    projects.map((project) => project.path),
  );
  return peekInboxList(projects, {
    assignedToMe: filters.assignedToMe,
    state: inboxFetchState(filters),
    search: "",
    linearHiddenTeamIds: loadHiddenLinearTeamIds(),
  });
}

function InboxSourceTab({
  source,
  selected,
  onSelect,
}: {
  source: InboxSource;
  selected: boolean;
  onSelect: (source: InboxSource) => void;
}) {
  const label = source === "linear" ? "Linear" : "GitHub";
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      onClick={() => onSelect(source)}
      className={`flex h-6 min-w-0 flex-1 items-center justify-center rounded-md px-2 text-[12px] leading-none ${
        selected
          ? "bg-content/10 text-content"
          : "text-content/50 hover:bg-content/5 hover:text-content"
      }`}
    >
      <span className="flex items-center gap-1.5">
        <InboxProviderMark
          provider={source}
          className="block size-3.5 shrink-0"
        />
        <span className="leading-none">{label}</span>
      </span>
    </button>
  );
}

function InboxDetailTab({
  label,
  selected,
  onSelect,
}: {
  label: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      onClick={onSelect}
      className={`relative flex h-9 items-center text-[12px] leading-none ${
        selected ? "text-content" : "text-content/50 hover:text-content"
      }`}
    >
      {label}
      {selected ? (
        <span className="absolute inset-x-0 bottom-0 h-0.5 bg-content" />
      ) : null}
    </button>
  );
}

type Props = {
  cwd: string;
  recents: RecentProject[];
  besideRail?: boolean;
  onClose?: () => void;
  onToggleSidebar?: () => void;
  onStart?: (item: InboxItem, body?: string) => void | Promise<void>;
};

export function InboxView({
  cwd,
  recents,
  besideRail = false,
  onClose,
  onToggleSidebar,
  onStart,
}: Props) {
  const listLock = useLockOverscroll<HTMLDivElement>();
  const detailLock = useLockOverscroll<HTMLDivElement>();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const logos = useTabGroupLogos();
  const [groupMascots] = useState(loadTabGroupMascots);
  const [groupColors] = useState(loadTabGroupColors);
  const [groupCustomColors] = useState(loadTabGroupCustomColors);

  const [searchInput, setSearchInput] = useState("");
  const [items, setItems] = useState<InboxItem[]>(
    () => peekInboxForRail(recents, cwd)?.items ?? [],
  );
  const [loading, setLoading] = useState(
    () => peekInboxForRail(recents, cwd) == null,
  );
  const [revalidating, setRevalidating] = useState(false);
  const [providerErrors, setProviderErrors] = useState<InboxProviderErrors>(
    () => peekInboxForRail(recents, cwd)?.errors ?? {},
  );
  const [refresh, setRefresh] = useState(0);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [filters, setFilters] = useState(loadInboxFilters);
  const [source, setSource] = useState(loadInboxSource);
  const [filterMenu, setFilterMenu] = useState<{ x: number; y: number } | null>(
    null,
  );
  const [linearHiddenTeamIds, setLinearHiddenTeamIds] = useState(
    loadHiddenLinearTeamIds,
  );
  const prevRefresh = useRef(refresh);

  const projects = useMemo(
    () => inboxProjectsForRail(recents, cwd),
    [cwd, recents],
  );
  const projectOptions = useMemo(
    () => inboxProjectOptions(projects, logos),
    [logos, projects],
  );
  const activeFilters = useMemo(
    () =>
      pruneInboxFilters(
        filters,
        projects.map((project) => project.path),
      ),
    [filters, projects],
  );
  const filtersActive = hasActiveInboxFilters(activeFilters, source);
  const fetchState = inboxFetchState(activeFilters);
  const fetchQuery = useMemo<InboxQuery>(
    () => ({
      assignedToMe: activeFilters.assignedToMe,
      state: fetchState,
      search: "",
      linearHiddenTeamIds,
    }),
    [activeFilters.assignedToMe, fetchState, linearHiddenTeamIds],
  );

  const resize = useDragResize({
    min: MIN_WIDTH,
    max: () => Math.min(MAX_WIDTH, Math.round(window.innerWidth * 0.5)),
    defaultWidth: DEFAULT_WIDTH,
    initial: rememberedWidth,
    onCommit: (width) => {
      rememberedWidth = width;
    },
  });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (filterMenu) {
        setFilterMenu(null);
        return;
      }
      onCloseRef.current?.();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [filterMenu]);

  useEffect(() => {
    const onChange = () => {
      setLinearHiddenTeamIds(loadHiddenLinearTeamIds());
      setRefresh((value) => value + 1);
    };
    window.addEventListener(LINEAR_CHANGE_EVENT, onChange);
    return () => window.removeEventListener(LINEAR_CHANGE_EVENT, onChange);
  }, []);

  useEffect(() => {
    const force = refresh !== prevRefresh.current;
    prevRefresh.current = refresh;
    const cached = peekInboxList(projects, fetchQuery);
    if (cached) {
      setItems(cached.items);
      setProviderErrors(cached.errors);
      setLoading(false);
    }
    if (!force && cached && inboxListIsFresh(projects, fetchQuery)) {
      return;
    }

    let cancelled = false;
    if (cached) setRevalidating(true);
    else {
      setLoading(true);
      setProviderErrors({});
    }
    void listInboxItems(projects, fetchQuery, { force })
      .then((next) => {
        if (cancelled) return;
        setItems(next.items);
        setProviderErrors(next.errors);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        if (cached) return;
        setItems([]);
        const message = err instanceof Error ? err.message : String(err);
        setProviderErrors({ github: message, linear: message });
      })
      .finally(() => {
        if (cancelled) return;
        setLoading(false);
        setRevalidating(false);
      });

    return () => {
      cancelled = true;
    };
  }, [fetchQuery, projects, refresh]);

  const visibleItems = useMemo(
    () =>
      applyInboxFilters(items, activeFilters, searchInput, Date.now(), source),
    [activeFilters, items, searchInput, source],
  );
  const inboxSeenTick = useInboxSeenTick();
  const sourceEntries = useMemo(
    () =>
      items
        .filter((item) => item.provider === source)
        .map((item) => ({
          key: inboxItemKey(item),
          updatedAt: item.updatedAt,
        })),
    [items, source],
  );
  const sourceHasUnseen = useMemo(
    () => sourceEntries.some(isInboxEntryUnseen),
    [inboxSeenTick, sourceEntries],
  );

  const searchNarrowed = searchInput.trim().length > 0;
  const narrowedByUser = searchNarrowed || filtersActive;
  const sourceError = providerErrors[source] ?? null;

  const selected =
    visibleItems.find((item) => inboxItemKey(item) === selectedKey) ??
    visibleItems[0] ??
    null;

  useEffect(() => {
    if (!selected) {
      setSelectedKey(null);
      return;
    }
    const key = inboxItemKey(selected);
    if (key !== selectedKey) setSelectedKey(key);
  }, [selected, selectedKey]);

  const onFiltersChange = (next: InboxFilters) => {
    const pruned = pruneInboxFilters(
      next,
      projects.map((project) => project.path),
    );
    setFilters(pruned);
    saveInboxFilters(pruned);
  };

  const onSourceChange = (next: InboxSource) => {
    setSource(next);
    saveInboxSource(next);
  };

  const onFilterButtonClick = (event: ReactMouseEvent<HTMLButtonElement>) => {
    if (filterMenu) {
      setFilterMenu(null);
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    setFilterMenu({
      x: rect.right - INBOX_FILTER_MENU_WIDTH,
      y: rect.bottom + 2,
    });
  };

  const list = (
    <div
      ref={resize.setPaneRef}
      className="relative flex h-full min-h-0 shrink-0 flex-col border-r border-content/10"
    >
      <div
        role="tablist"
        aria-label="Inbox source"
        className="flex h-9 shrink-0 items-center gap-px border-b border-content/10 px-2"
      >
        <InboxSourceTab
          source="github"
          selected={source === "github"}
          onSelect={onSourceChange}
        />
        <InboxSourceTab
          source="linear"
          selected={source === "linear"}
          onSelect={onSourceChange}
        />
      </div>
      <div className="flex h-9 shrink-0 items-center gap-1 border-b border-content/10 px-2">
        <div className="relative flex h-7 min-w-0 flex-1 items-center">
          <Search className="pointer-events-none absolute left-2 size-3 shrink-0 opacity-50" />
          <input
            value={searchInput}
            onChange={(event) => setSearchInput(event.target.value)}
            placeholder="Filter inbox"
            aria-label="Filter inbox"
            spellCheck={false}
            autoComplete="off"
            className="h-7 w-full rounded-md bg-transparent pl-7 pr-2 text-[12px] text-content outline-none placeholder:text-content/40"
          />
        </div>
        <button
          type="button"
          title="Filter inbox"
          aria-label="Filter inbox"
          aria-expanded={!!filterMenu}
          aria-haspopup="menu"
          onClick={onFilterButtonClick}
          className={`grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content ${
            filterMenu || filtersActive ? "bg-content/10 text-content" : ""
          }`}
        >
          <ListFilter className="size-3" strokeWidth={1.75} />
        </button>
        <button
          type="button"
          title="Mark all as read"
          aria-label="Mark all as read"
          disabled={!sourceHasUnseen}
          onClick={() => markInboxItemsSeen(sourceEntries)}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content disabled:cursor-default disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-content/45"
        >
          <CheckCheck className="size-3.5" strokeWidth={1.75} />
        </button>
        <button
          type="button"
          aria-label="Refresh"
          onClick={() => setRefresh((value) => value + 1)}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
        >
          {loading || revalidating ? (
            <LoaderCircle
              className="size-3.5 animate-spin"
              strokeWidth={1.75}
            />
          ) : (
            <RefreshCw className="size-3.5" strokeWidth={1.75} />
          )}
        </button>
      </div>
      <div
        ref={listLock}
        className="min-h-0 flex-1 overflow-y-auto overscroll-none"
      >
        {sourceError && visibleItems.length === 0 ? (
          <p className="px-3 py-2 text-[12px] text-content/50">{sourceError}</p>
        ) : loading && items.length === 0 ? (
          <div className="flex justify-center py-10 text-content/40">
            <LoaderCircle className="size-4 animate-spin" strokeWidth={1.75} />
          </div>
        ) : visibleItems.length === 0 ? (
          <p className="px-3 py-2 text-[12px] text-content/50">
            {narrowedByUser
              ? searchNarrowed
                ? source === "linear"
                  ? "No matching Linear issues"
                  : "No matching issues or pull requests"
                : source === "linear"
                  ? "No Linear issues match these filters"
                  : "No issues or pull requests match these filters"
              : source === "linear"
                ? "No Linear issues"
                : projects.length === 0
                  ? "Open a project to fill the inbox"
                  : "No matching issues or pull requests"}
          </p>
        ) : (
          <ul className="flex flex-col gap-0.5 p-1.5">
            {visibleItems.map((item) => {
              const key = inboxItemKey(item);
              const projectKey = projectName(item.projectPath);
              return (
                <li key={key}>
                  <InboxCard
                    item={item}
                    active={selected != null && key === inboxItemKey(selected)}
                    logoPath={resolveTabGroupLogo(projectKey, logos)}
                    mascotName={resolveTabGroupMascot(projectKey, groupMascots)}
                    mascotColor={resolveTabGroupColor(
                      projectKey,
                      groupColors,
                      groupCustomColors,
                      projectKey,
                    )}
                    onSelect={() => {
                      markInboxItemSeen({
                        key,
                        updatedAt: item.updatedAt,
                      });
                      setSelectedKey(key);
                    }}
                  />
                </li>
              );
            })}
          </ul>
        )}
      </div>
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize inbox list"
        className={`absolute inset-y-0 -right-px z-10 w-1.5 cursor-col-resize touch-none ${
          resize.dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
    </div>
  );

  const filtersPortal = filterMenu ? (
    <InboxFiltersMenu
      x={filterMenu.x}
      y={filterMenu.y}
      projects={projectOptions}
      source={source}
      filters={activeFilters}
      onChange={onFiltersChange}
      onClose={() => setFilterMenu(null)}
    />
  ) : null;

  return (
    <div
      role="region"
      aria-label="Inbox"
      data-app-inbox
      className="flex min-h-0 min-w-0 flex-1 flex-col text-content"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center border-b border-content/10"
        data-tauri-drag-region="deep"
      >
        {IS_MAC && !besideRail ? <div className="w-[78px] shrink-0" /> : null}
        {besideRail ? null : (
          <OverlayNav onBack={onClose} onToggleSidebar={onToggleSidebar} />
        )}
        <div className="flex min-w-0 flex-1 items-center gap-2 px-3 text-[13px]">
          <Inbox
            className="size-3.5 shrink-0 text-content/45"
            strokeWidth={1.75}
          />
          <span className="min-w-0 truncate text-content">Inbox</span>
        </div>
        {IS_MAC ? null : <WindowControls />}
      </div>

      <div className="flex min-h-0 min-w-0 flex-1">
        {list}
        <div
          ref={detailLock}
          className="min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-none"
        >
          <InboxDetailBody
            item={selected}
            cwd={cwd}
            projects={projectOptions}
            revision={refresh}
            onStart={onStart}
          />
        </div>
      </div>
      {filtersPortal}
    </div>
  );
}

function InboxDetailBody({
  item,
  cwd,
  projects,
  revision = 0,
  onStart,
}: {
  item: InboxItem | null;
  cwd: string;
  projects: InboxProjectOption[];
  revision?: number;
  onStart?: (item: InboxItem, body?: string) => void | Promise<void>;
}) {
  if (!item) {
    return (
      <div className="flex h-full flex-col items-center justify-center px-6 text-center">
        <Inbox className="mb-3 size-6 text-content/30" strokeWidth={1.75} />
        <p className="text-[13px] text-content/45">
          Select an issue or pull request
        </p>
      </div>
    );
  }
  return (
    <InboxDetail
      key={inboxItemKey(item)}
      item={item}
      cwd={cwd}
      projects={projects}
      revision={revision}
      onStart={onStart}
    />
  );
}

type InboxStatusMark = {
  Icon: IconComponent;
  className: string;
  label: string;
};

/** Status reads from the glyph first and the color second, so it survives color blindness. */
function inboxStatusMark(item: InboxItem): InboxStatusMark {
  const label = inboxItemStatus(item);
  const pr = item.kind === "pr";
  if (label === "Draft") {
    return {
      Icon: GitPullRequestDraft,
      className: "text-content/50",
      label,
    };
  }
  if (label === "Merged") {
    return { Icon: GitMerge, className: "text-violet-400/90", label };
  }
  if (label === "Closed") {
    return {
      Icon: pr ? GitPullRequestClosed : CircleX,
      className: "text-rose-400/90",
      label,
    };
  }
  return {
    Icon: pr ? GitPullRequest : CircleDot,
    className: "text-emerald-400/90",
    label,
  };
}

function InboxCard({
  item,
  active,
  logoPath,
  mascotName,
  mascotColor,
  onSelect,
}: {
  item: InboxItem;
  active: boolean;
  logoPath: string | null;
  mascotName: string | null;
  mascotColor: string;
  onSelect: () => void;
}) {
  useInboxSeenTick();
  const status = inboxStatusMark(item);
  const kindLabel = item.kind === "pr" ? "Pull request" : "Issue";
  const time = formatRelativeTime(item.updatedAt);
  const name = projectName(item.projectPath);
  const linear = item.provider === "linear";
  const source = linear ? item.teamName || item.repo : item.repo || name;
  const unseen = isInboxEntryUnseen({
    key: inboxItemKey(item),
    updatedAt: item.updatedAt,
  });

  return (
    <button
      type="button"
      title={item.title}
      aria-current={active ? "true" : undefined}
      aria-label={`${status.label} ${kindLabel.toLowerCase()} ${inboxItemRef(
        item,
      )}: ${item.title}${unseen ? ", new" : ""}`}
      onClick={onSelect}
      className={`flex w-full flex-col rounded-md border px-2.5 py-2 text-left ${
        active
          ? "border-transparent bg-content/10 text-content"
          : "border-transparent text-content/80 hover:bg-content/5 hover:text-content"
      }`}
    >
      <span className="flex items-center gap-2">
        <span className="flex min-w-0 flex-1 items-center gap-1.5">
          <InboxProviderMark
            provider={item.provider}
            className="size-3.5 shrink-0"
          />
          <status.Icon
            className={`size-3 shrink-0 ${status.className}`}
            strokeWidth={1.75}
          />
          <span className="min-w-0 truncate text-[11px] text-content/50">
            {kindLabel} · {inboxItemRef(item)}
          </span>
        </span>
        {time || unseen ? (
          <span className="flex shrink-0 items-center gap-1.5">
            {time ? (
              <span className="text-[11px] tabular-nums text-content/45">
                {time}
              </span>
            ) : null}
            {unseen ? (
              <span aria-hidden className="size-1.5 rounded-full bg-accent" />
            ) : null}
          </span>
        ) : null}
      </span>
      <span className="mt-1 line-clamp-1 text-[13px] font-semibold leading-snug text-content">
        {item.title}
      </span>
      <span className="mt-1 flex min-w-0 items-center gap-2">
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-[11px] text-content/45">
          {linear ? null : logoPath ? (
            <ProjectLogoIcon
              path={logoPath}
              className="size-3.5 shrink-0 rounded-sm"
              imageClassName="size-3.5"
            />
          ) : (
            <ProjectMascot
              project={name}
              color={mascotColor}
              name={mascotName}
              className="size-3 shrink-0"
            />
          )}
          <span className="min-w-0 truncate">{source}</span>
        </span>
        {item.labels.length > 0 ? (
          <span className="flex min-w-0 shrink-0 items-center gap-1">
            {item.labels.slice(0, 2).map((label) => (
              <InboxLabel key={label.name} label={label} compact />
            ))}
          </span>
        ) : null}
      </span>
    </button>
  );
}

function InboxDetail({
  item,
  cwd,
  projects,
  revision,
  onStart,
}: {
  item: InboxItem;
  cwd: string;
  projects: InboxProjectOption[];
  revision: number;
  onStart?: (item: InboxItem, body?: string) => void | Promise<void>;
}) {
  const linear = item.provider === "linear";
  const isPr = !linear && item.kind === "pr";
  const githubKind =
    item.kind === "issue" || item.kind === "pr" ? item.kind : null;
  const cached = linear
    ? peekLinearIssueDetails(item.id ?? "")
    : githubKind
      ? peekGithubWorkItemDetails(item.projectPath, githubKind, item.number)
      : null;
  const cachedDiff = isPr
    ? peekGithubPrDiff(item.projectPath, item.number)
    : null;
  const cachedThread = linear
    ? peekLinearIssueThread(item.id ?? "")
    : githubKind
      ? peekGithubWorkItemThread(item.projectPath, githubKind, item.number)
      : null;
  const [details, setDetails] = useState<GithubWorkItemDetails | null>(cached);
  const [loading, setLoading] = useState(cached == null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"summary" | "code">("summary");
  const [prDiff, setPrDiff] = useState<GithubPrDiff | null>(cachedDiff);
  const [diffLoading, setDiffLoading] = useState(isPr && cachedDiff == null);
  const [diffError, setDiffError] = useState<string | null>(null);
  const [thread, setThread] = useState<
    GithubWorkItemThread | LinearIssueThread | null
  >(cachedThread);
  const [threadLoading, setThreadLoading] = useState(cachedThread == null);
  const [threadError, setThreadError] = useState<string | null>(null);
  const [replyTo, setReplyTo] = useState<InboxReplyTarget | null>(null);
  const [posting, setPosting] = useState(false);
  const [postError, setPostError] = useState<string | null>(null);
  const defaultProject =
    projects.find((project) => sameProjectPath(project.path, cwd))?.path ??
    projects[0]?.path ??
    cwd;
  const [startProject, setStartProject] = useState(defaultProject);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const status = linear
    ? item.state || inboxItemStatus(item)
    : inboxItemStatus(item);
  const statusMark = inboxStatusMark(item);

  const source = linear
    ? item.teamName || item.repo
    : item.repo || projectName(item.projectPath);
  const markdownCwd = linear ? startProject || cwd : item.projectPath || cwd;
  const authorName = details?.author?.trim() ?? "";
  const extraAssignees = item.assignees.filter(
    (person) =>
      !authorName ||
      person.login.trim().toLowerCase() !== authorName.toLowerCase(),
  );
  const showAssignment =
    extraAssignees.length > 0 || item.assignees.length === 0;
  const reviewDecision =
    details?.reviewDecision?.trim() || thread?.reviewDecision?.trim() || "";
  const reviewLabel = githubReviewDecisionLabel(reviewDecision);
  const reviewClass =
    reviewDecision.toUpperCase() === "APPROVED"
      ? "text-emerald-400/90"
      : reviewDecision.toUpperCase() === "CHANGES_REQUESTED"
        ? "text-rose-400/90"
        : "text-content/50";
  const baseRef =
    details?.baseRefName?.trim() || thread?.baseRefName?.trim() || "";
  const headRef =
    details?.headRefName?.trim() || thread?.headRefName?.trim() || "";

  useEffect(() => {
    let cancelled = false;
    const cachedDetails = linear
      ? peekLinearIssueDetails(item.id ?? "")
      : githubKind
        ? peekGithubWorkItemDetails(item.projectPath, githubKind, item.number)
        : null;
    if (cachedDetails) {
      setDetails(cachedDetails);
      setLoading(false);
      setError(null);
    } else {
      setLoading(true);
      setError(null);
      setDetails(null);
    }
    const pending = linear
      ? item.id
        ? linearIssueDetails(item.id)
        : Promise.reject(new Error("Missing Linear issue"))
      : githubKind
        ? githubWorkItemDetails(item.projectPath, githubKind, item.number)
        : Promise.reject(new Error("Unknown inbox item"));
    void pending
      .then((next) => {
        if (cancelled) return;
        setDetails(next);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        if (cachedDetails) return;
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [githubKind, item.id, item.number, item.projectPath, linear, revision]);

  useEffect(() => {
    let cancelled = false;
    if (linear) {
      const id = item.id ?? "";
      const cachedThread = peekLinearIssueThread(id);
      if (cachedThread) {
        setThread(cachedThread);
        setThreadLoading(false);
        setThreadError(null);
      } else {
        setThreadLoading(true);
        setThreadError(null);
        setThread(null);
      }
      void linearIssueThread(id)
        .then((next) => {
          if (cancelled) return;
          setThread(next);
          setThreadError(null);
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          if (cachedThread) return;
          setThreadError(err instanceof Error ? err.message : String(err));
        })
        .finally(() => {
          if (!cancelled) setThreadLoading(false);
        });
      return () => {
        cancelled = true;
      };
    }
    if (!githubKind) return;
    const cachedThread = peekGithubWorkItemThread(
      item.projectPath,
      githubKind,
      item.number,
    );
    if (cachedThread) {
      setThread(cachedThread);
      setThreadLoading(false);
      setThreadError(null);
    } else {
      setThreadLoading(true);
      setThreadError(null);
      setThread(null);
    }
    void githubWorkItemThread(item.projectPath, githubKind, item.number)
      .then((next) => {
        if (cancelled) return;
        setThread(next);
        setThreadError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        if (cachedThread) return;
        setThreadError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setThreadLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [githubKind, item.id, item.number, item.projectPath, linear, revision]);

  useEffect(() => {
    if (!isPr || tab !== "code") return;
    let cancelled = false;
    const cachedDiff = peekGithubPrDiff(item.projectPath, item.number);
    if (cachedDiff) {
      setPrDiff(cachedDiff);
      setDiffLoading(false);
      setDiffError(null);
    } else {
      setDiffLoading(true);
      setDiffError(null);
      setPrDiff(null);
    }
    void githubPrDiff(item.projectPath, item.number)
      .then((next) => {
        if (cancelled) return;
        setPrDiff(next);
        setDiffError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        if (cachedDiff) return;
        setDiffError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setDiffLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isPr, item.number, item.projectPath, revision, tab]);

  const postComment = async (body: string) => {
    setPosting(true);
    setPostError(null);
    try {
      if (linear) {
        const id = item.id ?? "";
        await linearIssueComment(id, body, { parentId: replyTo?.id });
        setReplyTo(null);
        try {
          setThread(await linearIssueThread(id, { force: true }));
        } catch (err: unknown) {
          setPostError(err instanceof Error ? err.message : String(err));
        }
        return;
      }
      if (!githubKind) throw new Error("Unknown inbox item");
      await githubWorkItemComment(
        item.projectPath,
        githubKind,
        item.number,
        body,
        { inReplyTo: replyTo?.threadId },
      );
      setReplyTo(null);
      try {
        setThread(
          await githubWorkItemThread(
            item.projectPath,
            githubKind,
            item.number,
            {
              force: true,
            },
          ),
        );
      } catch (err: unknown) {
        setPostError(err instanceof Error ? err.message : String(err));
      }
    } catch (err: unknown) {
      setPostError(err instanceof Error ? err.message : String(err));
      throw err;
    } finally {
      setPosting(false);
    }
  };

  return (
    <div className={`mx-auto flex w-full flex-col gap-5 px-8 py-8 max-w-5xl`}>
      <header className="flex flex-col gap-3">
        <div className="flex items-center gap-2 text-[12px] text-content/50">
          <InboxProviderMark provider={item.provider} className="size-3.5" />
          <span>{item.kind === "pr" ? "Pull request" : "Issue"}</span>
          <span className="tabular-nums">{inboxItemRef(item)}</span>
          <span className={`flex items-center gap-1 ${statusMark.className}`}>
            <statusMark.Icon className="size-3.5" strokeWidth={1.75} />
            {status}
          </span>
          {source ? <span className="truncate">{source}</span> : null}
        </div>
        <h1 className="text-[20px] font-semibold leading-tight text-content">
          {item.title}
        </h1>
        <div className="flex flex-wrap items-center gap-2 text-[12px] text-content/50">
          {authorName ? (
            <InboxPerson
              name={authorName}
              avatarUrl={inboxPersonAvatarUrl(
                item.provider,
                authorName,
                details?.authorAvatarUrl,
              )}
              size={16}
            />
          ) : null}
          {showAssignment ? (
            <>
              {authorName ? <span aria-hidden>·</span> : null}
              {extraAssignees.length > 0 ? (
                <span className="flex min-w-0 flex-wrap items-center gap-2">
                  {extraAssignees.map((person) => (
                    <InboxPerson
                      key={person.login}
                      name={person.login}
                      avatarUrl={inboxPersonAvatarUrl(
                        item.provider,
                        person.login,
                        person.avatarUrl,
                      )}
                      size={16}
                    />
                  ))}
                </span>
              ) : (
                <span>Unassigned</span>
              )}
            </>
          ) : null}
          {linear ? null : (
            <>
              <span aria-hidden>·</span>
              <span>{projectName(item.projectPath)}</span>
            </>
          )}
          {formatRelativeTime(item.updatedAt) ? (
            <>
              <span aria-hidden>·</span>
              <span>Updated {formatRelativeTime(item.updatedAt)}</span>
            </>
          ) : null}
          {baseRef && headRef ? (
            <>
              <span aria-hidden>·</span>
              <span className="inline-flex min-w-0 items-center gap-1">
                <GitCompare className="size-3 shrink-0" strokeWidth={1.75} />
                <span className="min-w-0 truncate">
                  {baseRef} ← {headRef}
                </span>
              </span>
            </>
          ) : null}
          {reviewLabel ? (
            <>
              <span aria-hidden>·</span>
              <span className={reviewClass}>{reviewLabel}</span>
            </>
          ) : null}
        </div>
        {item.labels.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {item.labels.map((label) => (
              <InboxLabel key={label.name} label={label} />
            ))}
          </div>
        ) : null}
        <div className="flex flex-wrap items-center gap-2 pt-1">
          {onStart && item.kind !== "pr" ? (
            <>
              <button
                type="button"
                disabled={
                  starting || (linear && (!startProject || loading || !!error))
                }
                onClick={() => {
                  if (starting) return;
                  setStarting(true);
                  setStartError(null);
                  const next = linear
                    ? { ...item, projectPath: startProject }
                    : item;
                  void Promise.resolve(
                    onStart(next, linear ? (details?.body ?? "") : undefined),
                  )
                    .catch((err: unknown) => {
                      setStartError(
                        err instanceof Error ? err.message : String(err),
                      );
                    })
                    .finally(() => setStarting(false));
                }}
                className="inline-flex items-center gap-1 rounded-md bg-content px-3 h-6.5 text-[12px] text-background-base hover:bg-content/80 disabled:cursor-default disabled:opacity-40"
              >
                {starting ? "Sending..." : "Send to agent"}
              </button>
              {linear ? (
                <InboxProjectPicker
                  projects={projects}
                  value={startProject}
                  onChange={setStartProject}
                />
              ) : null}
            </>
          ) : null}
          <button
            type="button"
            onClick={() => void openUrl(item.url)}
            className={
              item.kind === "pr"
                ? "inline-flex items-center gap-1.5 rounded-md bg-content px-3 h-7 text-[12px] text-background-base hover:bg-content/80"
                : "inline-flex items-center gap-1.5 rounded-md px-3 h-7 text-[12px] text-content/70 hover:bg-content/10 hover:text-content"
            }
          >
            <ExternalLink className="size-3.5" strokeWidth={1.75} />
            {item.kind === "pr"
              ? "Review on GitHub"
              : linear
                ? "Open in Linear"
                : "Open on GitHub"}
          </button>
        </div>
        {startError ? (
          <p className="text-[12px] text-red-400/90">{startError}</p>
        ) : null}
      </header>
      {isPr ? (
        <div
          role="tablist"
          aria-label="Pull request sections"
          className="flex h-9 gap-4 items-stretch border-b border-content/10"
        >
          <InboxDetailTab
            label="Summary"
            selected={tab === "summary"}
            onSelect={() => setTab("summary")}
          />
          <InboxDetailTab
            label="Code"
            selected={tab === "code"}
            onSelect={() => setTab("code")}
          />
        </div>
      ) : (
        <div className="border-t border-content/10" />
      )}
      {isPr && tab === "code" ? (
        diffLoading ? (
          <div className="flex justify-center py-10 text-content/40">
            <LoaderCircle className="size-4 animate-spin" strokeWidth={1.75} />
          </div>
        ) : diffError ? (
          <p className="text-[13px] text-content/50">{diffError}</p>
        ) : prDiff ? (
          <InboxPrDiff
            key={`${item.projectPath}:${item.number}:${revision}`}
            diff={prDiff}
          />
        ) : (
          <p className="text-[13px] text-content/45">No file changes</p>
        )
      ) : loading ? (
        <div className="flex justify-center py-10 text-content/40">
          <LoaderCircle className="size-4 animate-spin" strokeWidth={1.75} />
        </div>
      ) : error ? (
        <p className="text-[13px] text-content/50">{error}</p>
      ) : (
        <>
          {details?.body.trim() ? (
            <AgentMarkdown
              text={details.body}
              cwd={markdownCwd}
              allowRemoteMedia
            />
          ) : (
            <p className="text-[13px] text-content/45">No description</p>
          )}
          <InboxComments
            thread={thread}
            loading={threadLoading}
            error={threadError}
            cwd={markdownCwd}
            provider={item.provider}
            replyMode={linear ? "parent" : "thread"}
            onReply={setReplyTo}
          />
          <InboxCommentForm
            replyTo={replyTo}
            posting={posting}
            error={postError}
            onCancelReply={() => {
              setReplyTo(null);
              setPostError(null);
            }}
            onSubmit={postComment}
          />
        </>
      )}
    </div>
  );
}

function InboxPerson({
  name,
  avatarUrl,
  size = 20,
  className = "",
}: {
  name: string;
  avatarUrl?: string;
  size?: number;
  className?: string;
}) {
  const [failed, setFailed] = useState(!avatarUrl);
  const initial = name.trim().charAt(0).toUpperCase() || "?";

  useEffect(() => {
    setFailed(!avatarUrl);
  }, [avatarUrl]);

  return (
    <span className={`inline-flex min-w-0 items-center gap-1.5 ${className}`}>
      {avatarUrl && !failed ? (
        <img
          src={avatarUrl}
          alt=""
          width={size}
          height={size}
          referrerPolicy="no-referrer"
          draggable={false}
          onError={() => setFailed(true)}
          className="shrink-0 rounded-full bg-content/10 object-cover"
          style={{ width: size, height: size }}
        />
      ) : (
        <span
          aria-hidden
          className="grid shrink-0 place-items-center rounded-full bg-content/12 font-medium text-content/55"
          style={{
            width: size,
            height: size,
            fontSize: Math.max(9, Math.round(size * 0.45)),
          }}
        >
          {initial}
        </span>
      )}
      <span className="min-w-0 truncate">{name}</span>
    </span>
  );
}

function InboxProjectPicker({
  projects,
  value,
  onChange,
}: {
  projects: InboxProjectOption[];
  value: string;
  onChange: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const selected =
    projects.find((project) => sameProjectPath(project.path, value)) ??
    projects[0] ??
    null;

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (button.current?.contains(target) || menu.current?.contains(target)) {
        return;
      }
      setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpen(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  return (
    <div className="relative">
      <button
        ref={button}
        type="button"
        disabled={projects.length === 0}
        onClick={() => setOpen((next) => !next)}
        className="inline-flex h-7 max-w-48 items-center gap-1.5 rounded-md border border-content/10 bg-content/5 px-2 text-[12px] text-content/80 hover:bg-content/10 hover:text-content disabled:cursor-default disabled:opacity-40"
      >
        {selected ? <InboxProjectMark project={selected} /> : null}
        <span className="min-w-0 truncate">
          {selected?.name ?? "Choose project"}
        </span>
        <ChevronDown
          className="size-3 shrink-0 text-content/45"
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <div
          ref={menu}
          role="listbox"
          className="absolute left-0 top-full z-30 mt-1 max-h-64 min-w-full max-w-64 overflow-y-auto rounded-lg border border-content/10 bg-content/10 p-1 shadow-xl backdrop-blur-xl outline-none"
        >
          {projects.map((project) => {
            const active = selected
              ? sameProjectPath(project.path, selected.path)
              : false;
            return (
              <button
                key={project.path}
                type="button"
                role="option"
                aria-selected={active}
                onClick={() => {
                  onChange(project.path);
                  setOpen(false);
                }}
                className={`flex h-7 w-full items-center gap-1.5 rounded-md px-2 text-left text-[12px] ${
                  active
                    ? "bg-content/10 text-content"
                    : "text-content/80 hover:bg-content/5 hover:text-content"
                }`}
              >
                <InboxProjectMark project={project} />
                <span className="min-w-0 truncate">{project.name}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function InboxLabel({
  label,
  compact = false,
}: {
  label: GithubLabel;
  compact?: boolean;
}) {
  const color = labelColor(label.color);
  return (
    <span
      className={`inline-flex min-w-0 items-center gap-1 rounded px-1.5 py-px text-content/50 bg-content/8 ${
        compact ? "max-w-20 text-[10px]" : "text-[11px]"
      }`}
    >
      {color ? (
        <span
          aria-hidden
          className="size-1.5 shrink-0 rounded-full"
          style={{ backgroundColor: color }}
        />
      ) : null}
      <span className="min-w-0 truncate">{label.name}</span>
    </span>
  );
}

function labelColor(value: string): string | null {
  const hex = value.trim().replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(hex)) return null;
  return `#${hex}`;
}
