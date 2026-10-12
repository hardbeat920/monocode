import { useEffect, useRef, useState, type ReactNode } from "react";
import { IconButton } from "../../../app/shell/TitleBar";
import {
  Clock,
  File,
  FileDiff,
  ListFilter,
  MessageMultiple,
  Plus,
  Search,
} from "../../../shared/ui/icons";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { artifactLabel, type ArtifactCard } from "../../artifacts/artifacts";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import type { MonoSpawnedSession, Session } from "../../sessions/model/session";
import type { MonoLook, MonoState } from "../model/mono";
import { HABITS_MAX } from "../model/monoHabits";
import { HabitPage } from "./HabitPage";
import { NewHabitPage } from "./NewHabitPage";
import { habitActions, HabitsList, useHabits } from "./MonoHabits";
import { ArtifactReader } from "../../artifacts/ui/ArtifactPanel";
import { ArtifactIcon } from "../../artifacts/ui/ArtifactCard";
import {
  LaunchedSessionPage,
  MonoSessionList,
  type ShownMonoSession,
} from "./MonoSessionList";
import { EmptyState } from "./monoPanelParts";
import { PanelStack, type StackPage } from "./PanelStack";
import { SessionFiltersMenu } from "../../sessions/ui/SessionFiltersMenu";
import {
  DEFAULT_SESSION_SIDEBAR_FILTERS,
  hasActiveSessionFilters,
  type SessionSidebarFilters,
} from "../../sessions/model/sessionFilters";
import { HARNESSES, type HarnessId } from "../../sessions/model/session";
import { MonoSidebar, MonoSidebarHeader } from "./MonoSidebar";
import type { SessionChangeTotals } from "../../sessions/ui/SessionReview";
import { MonoChangesView, type MonoChangesRequest } from "./MonoChangesView";

export type MonoOverviewTab = "sessions" | "habits" | "documents" | "changes";

const TABS: { id: MonoOverviewTab; label: string; icon: typeof Clock }[] = [
  { id: "sessions", label: "Sessions", icon: MessageMultiple },
  { id: "documents", label: "Documents", icon: File },
  { id: "habits", label: "Habits", icon: Clock },
  { id: "changes", label: "Changes", icon: FileDiff },
];

/** The conversation's recorded changes, reviewed and committed on their tab. */
export type MonoOverviewChanges = {
  sessionId: string;
  cwd: string;
  /** Null while the conversation has no changes. */
  totals: SessionChangeTotals | null;
  request: MonoChangesRequest;
  textHarness?: HarnessId;
  /** The projects the Mono works on. */
  projects: readonly string[];
};

type Route = { kind: "new-habit" } | { kind: "habit"; id: string };

type Props = {
  open: boolean;
  monoId: string;
  /** Its conversation's folder, which habits run in. */
  cwd: string;
  agent: MonoLook;
  state: MonoState;
  /** Every session it has started, oldest first. */
  launches: readonly MonoSpawnedSession[];
  sessions: readonly Session[];
  history?: readonly SessionSummary[];
  /** Documents it has written, oldest first. */
  documents: readonly ArtifactCard[];
  changes?: MonoOverviewChanges;
  tab: MonoOverviewTab;
  onTabChange: (tab: MonoOverviewTab) => void;
  onOpenSession: (sessionId: string) => void | Promise<void>;
  /** A session it started, open over the list until Back. */
  shownSession?: ShownMonoSession;
  onBackFromSession?: () => void;
  onOpenSessionInWorkspace?: (sessionId: string) => void | Promise<void>;
  /** A document it wrote, open over the list until Back. */
  shownDocumentId?: string;
  onBackFromDocument?: () => void;
  onOpenFile?: (path: string) => void;
  onOpenDocument: (id: string) => void;
  onClose: () => void;
  windowControls?: ReactNode;
};

/**
 * The Mono's work at a glance: the sessions it started, its habits and the
 * documents it wrote. Who it is and how it works live in its settings.
 */
export function MonoOverview({
  open,
  monoId,
  cwd,
  agent,
  state,
  launches,
  sessions,
  history,
  documents,
  changes,
  tab,
  onTabChange,
  onOpenSession,
  shownSession,
  onBackFromSession = () => {},
  onOpenSessionInWorkspace = () => {},
  shownDocumentId,
  onBackFromDocument = () => {},
  onOpenFile,
  onOpenDocument,
  onClose,
  windowControls,
}: Props) {
  const lock = useLockOverscroll<HTMLDivElement>();
  const habits = useHabits(monoId, state.status);
  const actions = habitActions(monoId);
  const [routes, setRoutes] = useState<Route[]>([]);
  const [query, setQuery] = useState("");
  const [filters, setFilters] = useState<SessionSidebarFilters>(
    DEFAULT_SESSION_SIDEBAR_FILTERS,
  );
  // Once opened, the changes stay mounted so a commit draft survives a look
  // at another tab.
  const [changesVisited, setChangesVisited] = useState(tab === "changes");
  if (tab === "changes" && !changesVisited) setChangesVisited(true);
  // Another Mono starts at its own front page, with all its sessions.
  useEffect(() => {
    setRoutes([]);
    setQuery("");
    setFilters(DEFAULT_SESSION_SIDEBAR_FILTERS);
  }, [monoId]);
  const push = (route: Route) => setRoutes((current) => [...current, route]);
  const back = () => setRoutes((current) => current.slice(0, -1));

  const pages: StackPage[] = routes.flatMap((route, depth): StackPage[] => {
    if (route.kind === "new-habit")
      return [
        {
          key: "new-habit",
          node: <NewHabitPage monoId={monoId} onBack={back} onCreated={back} />,
        },
      ];
    // A habit removed while open closes its page.
    const habit = habits?.find((entry) => entry.id === route.id);
    if (!habit) return [];
    return [
      {
        key: `habit:${route.id}:${depth}`,
        node: (
          <HabitPage
            habit={habit}
            color={agent.color}
            cwd={cwd}
            onBack={back}
            onRunNow={() => actions.runNow(habit.id)}
            onToggle={() => actions.toggle(habit.id)}
            onRemove={() => {
              actions.remove(habit.id);
              back();
            }}
          />
        ),
      },
    ];
  });

  // What the overview opened goes on top of everything, the way it would
  // from the list beneath.
  if (shownSession)
    pages.push({
      key: `session:${shownSession.sessionId}`,
      node: (
        <LaunchedSessionPage
          title={shownSession.title}
          onBack={onBackFromSession}
          onOpenInWorkspace={() =>
            onOpenSessionInWorkspace(shownSession.sessionId)
          }
          onClose={onClose}
        >
          {shownSession.pane}
        </LaunchedSessionPage>
      ),
    });
  if (shownDocumentId)
    pages.push({
      key: `document:${shownDocumentId}`,
      node: (
        <div className="flex min-h-0 flex-1 flex-col" data-mono-document-page>
          <ArtifactReader
            key={shownDocumentId}
            id={shownDocumentId}
            onBack={onBackFromDocument}
            onClose={onClose}
            onOpenFile={onOpenFile}
          />
        </div>
      ),
    });

  const habitsFull = !habits || habits.length >= HABITS_MAX;
  const counts: Record<MonoOverviewTab, number | undefined> = {
    sessions: launches.length,
    habits: habits?.length,
    documents: documents.length,
    changes: changes?.totals?.files,
  };

  return (
    <MonoSidebar
      open={open}
      kind="details"
      label={`${agent.name} overview`}
      color={agent.color}
      windowControls={windowControls}
    >
      <PanelStack pages={pages}>
        {/* Pages replace this header too, keeping their back button at the top. */}
        <MonoSidebarHeader
          title="Overview"
          heading={
            <OverviewTabs
              tab={tab}
              counts={counts}
              onChange={onTabChange}
            />
          }
          onClose={onClose}
          actions={
            tab === "habits" ? (
              <IconButton
                label={
                  habits && habits.length >= HABITS_MAX
                    ? `At most ${HABITS_MAX} habits`
                    : "New habit"
                }
                disabled={habitsFull}
                onClick={() => push({ kind: "new-habit" })}
              >
                <Plus className="size-3.5" strokeWidth={1.75} />
              </IconButton>
            ) : null
          }
        />
        {tab === "sessions" && launches.length ? (
          <SessionSearchBar
            query={query}
            onQueryChange={setQuery}
            filters={filters}
            onFiltersChange={setFilters}
            harnesses={HARNESSES.filter((harness) =>
              launches.some((launch) => launch.harness === harness),
            )}
          />
        ) : null}
        {changes && changesVisited ? (
          <div
            role="tabpanel"
            aria-label="Changes"
            hidden={tab !== "changes"}
            className="flex min-h-0 flex-1 flex-col"
            data-mono-overview={tab === "changes" ? "changes" : undefined}
          >
            <MonoChangesView
              sessionId={changes.sessionId}
              cwd={changes.cwd}
              request={changes.request}
              textHarness={changes.textHarness}
              agentName={agent.name}
              attachedProjects={changes.projects}
            />
          </div>
        ) : null}
        <div
          ref={lock}
          role="tabpanel"
          aria-label={TABS.find((entry) => entry.id === tab)?.label}
          hidden={tab === "changes"}
          className="min-h-0 flex-1 overflow-y-auto overscroll-none px-2 py-2"
          data-mono-overview={tab === "changes" ? undefined : tab}
        >
          {tab === "sessions" ? (
            launches.length ? (
              // Cards carry their own inset; this lines their icons up with
              // the tabs above, as the habit and document rows do.
              <div className="-mx-0.5">
                <MonoSessionList
                  agent={agent}
                  launches={[...launches].reverse()}
                  sessions={sessions}
                  history={history}
                  query={query}
                  filters={filters}
                  onOpenSession={onOpenSession}
                />
              </div>
            ) : (
              <EmptyState icon={MessageMultiple}>
                Ask {agent.name} to start a session on one of your projects,
                like a bug fix or a review, and it shows up here.
              </EmptyState>
            )
          ) : tab === "habits" ? (
            <HabitsList
              habits={habits}
              actions={actions}
              onOpen={(id) => push({ kind: "habit", id })}
              empty={
                <EmptyState icon={Clock}>
                  Ask {agent.name} to put recurring work on a schedule, like a
                  morning digest or a weekly report.
                </EmptyState>
              }
            />
          ) : documents.length ? (
            <ul className="flex flex-col gap-px">
              {[...documents].reverse().map((card) => (
                <li key={card.id}>
                  <DocumentRow card={card} onOpen={onOpenDocument} />
                </li>
              ))}
            </ul>
          ) : (
            <EmptyState icon={File}>
              Ask {agent.name} to write something up, like a plan or a summary,
              and it shows up here.
            </EmptyState>
          )}
        </div>
      </PanelStack>
    </MonoSidebar>
  );
}

/** The sidebar's session search and filters, over the Mono's sessions. */
function SessionSearchBar({
  query,
  onQueryChange,
  filters,
  onFiltersChange,
  harnesses,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  filters: SessionSidebarFilters;
  onFiltersChange: (filters: SessionSidebarFilters) => void;
  harnesses: HarnessId[];
}) {
  const filterButton = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const active = hasActiveSessionFilters(filters);
  return (
    <div className="flex h-9 shrink-0 items-center gap-1 border-b border-stroke px-2">
      <div className="relative flex h-7 min-w-0 flex-1 items-center">
        <Search className="pointer-events-none absolute left-2 size-3 shrink-0 opacity-50" />
        <input
          type="text"
          value={query}
          placeholder="Search sessions..."
          aria-label="Search sessions"
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          onChange={(event) => onQueryChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Escape" || !query) return;
            event.preventDefault();
            event.stopPropagation();
            onQueryChange("");
          }}
          className="h-full w-full min-w-0 rounded-md bg-transparent py-0 pl-7 pr-2 text-[12px] text-content outline-none placeholder:text-content/35"
        />
      </div>
      <button
        ref={filterButton}
        type="button"
        title="Filter sessions"
        aria-label="Filter sessions"
        aria-expanded={!!menu}
        aria-haspopup="menu"
        onClick={() => {
          if (menu) return setMenu(null);
          const rect = filterButton.current?.getBoundingClientRect();
          if (rect) setMenu({ x: rect.right - 228, y: rect.bottom + 2 });
        }}
        className={`grid size-6 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content ${
          menu || active ? "bg-selection text-content" : ""
        }`}
      >
        <ListFilter className="size-3" strokeWidth={1.75} />
      </button>
      {menu ? (
        <SessionFiltersMenu
          x={menu.x}
          y={menu.y}
          harnesses={harnesses}
          unavailableOption
          filters={filters}
          onChange={onFiltersChange}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}

/**
 * The overview's tabs: the open one names itself and how many it holds, the
 * others are just their icons.
 */
function OverviewTabs({
  tab,
  counts,
  onChange,
}: {
  tab: MonoOverviewTab;
  counts: Record<MonoOverviewTab, number | undefined>;
  onChange: (tab: MonoOverviewTab) => void;
}) {
  return (
    <div
      role="tablist"
      aria-label="Overview"
      className="-ml-2 flex items-center gap-0.5"
    >
      {TABS.map(({ id, label, icon: Icon }) => {
        const selected = id === tab;
        const count = counts[id];
        return (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-label={label}
            title={label}
            data-tauri-drag-region="false"
            onClick={() => onChange(id)}
            className={`flex h-7 items-center rounded-lg px-2 text-[12px] transition-colors duration-200 ${
              selected
                ? "bg-content/8 text-content"
                : "text-content/45 hover:bg-content/5 hover:text-content/80"
            }`}
          >
            {/* Folded tabs dot their icon, as the rail does, when they hold something new. */}
            {id === "changes" && !selected && count ? (
              <span className="relative grid shrink-0" data-changes-dot>
                <Icon
                  className="overview-tab-icon-with-dot size-3.5"
                  strokeWidth={1.75}
                />
                <span
                  aria-hidden
                  className="absolute -top-0.5 left-2.5 size-1.5 rounded-full bg-accent"
                />
              </span>
            ) : (
              <Icon className="size-3.5 shrink-0" strokeWidth={1.75} />
            )}
            {/* The name and count open out of the icon, and fold back into it. */}
            <span
              aria-hidden={!selected}
              data-tab-label={selected ? "open" : "closed"}
              className={`grid transition-[grid-template-columns,opacity] duration-200 ease-out motion-reduce:transition-none ${
                selected
                  ? "grid-cols-[1fr] opacity-100"
                  : "grid-cols-[0fr] opacity-0"
              }`}
            >
              <span className="flex min-w-0 items-center gap-1.5 overflow-hidden whitespace-nowrap">
                <span className="pl-1.5">{label}</span>
                {count ? (
                  <span className="min-w-4 rounded bg-accent px-1 text-center text-[10px] font-medium leading-4 tabular-nums text-white">
                    {count}
                  </span>
                ) : null}
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** A document it wrote, laid out like a habit; opens in the reader. */
function DocumentRow({
  card,
  onOpen,
}: {
  card: ArtifactCard;
  onOpen: (id: string) => void;
}) {
  const label = artifactLabel(card.kind);
  return (
    <button
      type="button"
      data-mono-document={card.id}
      aria-label={`Open ${label.toLowerCase()} ${card.title}`}
      onClick={() => onOpen(card.id)}
      className="flex w-full min-w-0 items-center gap-2.5 rounded-md px-2 py-2 text-left hover:bg-content/5"
    >
      <span
        aria-hidden
        className="grid size-7 shrink-0 place-items-center rounded-md bg-content/6 text-content/60"
      >
        <ArtifactIcon kind={card.kind} className="size-3.5" />
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[12px] leading-5 text-content/85">
          {card.title}
        </span>
        <span className="truncate text-[11px] leading-4 text-content/40">
          {card.summary || label}
        </span>
      </span>
    </button>
  );
}
