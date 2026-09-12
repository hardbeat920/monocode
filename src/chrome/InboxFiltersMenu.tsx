import { Check, CircleDot, GitPullRequest } from "./icons";
import { type ReactNode } from "react";
import type { InboxKind } from "../lib/githubTasks";
import {
  DEFAULT_INBOX_FILTERS,
  hasActiveInboxFilters,
  type InboxFilters,
  type InboxSource,
  type InboxTimeFilter,
  type LinearProjectOption,
} from "../lib/inboxFilters";
import type { LinearTeam } from "../lib/linear";
import { Popover } from "./Popover";
import { ProjectLogoIcon } from "./ProjectLogoIcon";

export const INBOX_FILTER_MENU_WIDTH = 228;

type ProjectOption = {
  path: string;
  name: string;
  logoPath: string | null;
};

type Props = {
  x: number;
  y: number;
  projects: ProjectOption[];
  linearProjects: LinearProjectOption[];
  linearTeams: LinearTeam[];
  hiddenLinearTeamIds: string[];
  source: InboxSource;
  filters: InboxFilters;
  onChange: (filters: InboxFilters) => void;
  /** Shared with Settings → Linear Teams; narrows the fetch, not just the list. */
  onLinearTeamsChange: (ids: string[]) => void;
  onClose: () => void;
};

const TIME_OPTIONS: { id: InboxTimeFilter; label: string }[] = [
  { id: "all", label: "All time" },
  { id: "today", label: "Today" },
  { id: "7d", label: "Last 7 days" },
  { id: "30d", label: "Last 30 days" },
];

const KIND_OPTIONS: {
  id: InboxKind;
  label: string;
  icon: ReactNode;
}[] = [
  {
    id: "issue",
    label: "Issues",
    icon: <CircleDot className="size-3.5 shrink-0" strokeWidth={1.75} />,
  },
  {
    id: "pr",
    label: "Pull requests",
    icon: <GitPullRequest className="size-3.5 shrink-0" strokeWidth={1.75} />,
  },
];

export function InboxFiltersMenu({
  x,
  y,
  projects,
  linearProjects,
  linearTeams,
  hiddenLinearTeamIds,
  source,
  filters,
  onChange,
  onLinearTeamsChange,
  onClose,
}: Props) {
  const hiddenProjects = new Set(filters.hiddenProjects);
  const hiddenLinearProjects = new Set(filters.hiddenLinearProjects);
  const hiddenTeams = new Set(hiddenLinearTeamIds);
  const hiddenKinds = new Set(filters.hiddenKinds);
  const teamsActive = source === "linear" && hiddenLinearTeamIds.length > 0;

  const toggleAssigned = () => {
    onChange({ ...filters, assignedToMe: !filters.assignedToMe });
  };

  const toggleKind = (kind: InboxKind) => {
    const next = new Set(hiddenKinds);
    if (next.has(kind)) next.delete(kind);
    else next.add(kind);
    onChange({ ...filters, hiddenKinds: [...next] });
  };

  const toggleProject = (path: string) => {
    const next = new Set(hiddenProjects);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    onChange({ ...filters, hiddenProjects: [...next] });
  };

  const toggleLinearTeam = (id: string) => {
    const next = new Set(hiddenTeams);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onLinearTeamsChange([...next]);
  };

  const toggleLinearProject = (id: string) => {
    const next = new Set(hiddenLinearProjects);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onChange({ ...filters, hiddenLinearProjects: [...next] });
  };

  const setTime = (time: InboxTimeFilter) => {
    onChange({ ...filters, time });
  };

  const toggleStatus = (key: keyof InboxFilters["status"]) => {
    onChange({
      ...filters,
      status: { ...filters.status, [key]: !filters.status[key] },
    });
  };

  return (
    <Popover
      anchor={{ x, y }}
      gap={0}
      width={INBOX_FILTER_MENU_WIDTH}
      maxHeight={480}
      onDismiss={onClose}
      role="menu"
      aria-label="Filter inbox"
      onContextMenu={(event) => event.preventDefault()}
      className="overflow-y-auto overscroll-none p-1"
    >
      <FilterItem
        label="Assigned to me"
        checked={filters.assignedToMe}
        onClick={toggleAssigned}
      />

      <SectionLabel>Status</SectionLabel>
      <FilterItem
        label="Open"
        checked={filters.status.open}
        onClick={() => toggleStatus("open")}
      />
      {source !== "linear" ? (
        <FilterItem
          label="Draft"
          checked={filters.status.draft}
          onClick={() => toggleStatus("draft")}
        />
      ) : null}
      <FilterItem
        label="Closed"
        checked={filters.status.closed}
        onClick={() => toggleStatus("closed")}
      />
      {source !== "linear" ? (
        <FilterItem
          label="Merged"
          checked={filters.status.merged}
          onClick={() => toggleStatus("merged")}
        />
      ) : null}

      <SectionLabel>Time</SectionLabel>
      {TIME_OPTIONS.map((option) => (
        <FilterItem
          key={option.id}
          label={option.label}
          checked={filters.time === option.id}
          onClick={() => setTime(option.id)}
        />
      ))}

      {source !== "linear" ? (
        <>
          <SectionLabel>Type</SectionLabel>
          {KIND_OPTIONS.map((option) => (
            <FilterItem
              key={option.id}
              label={
                source === "gitlab" && option.id === "pr"
                  ? "Merge requests"
                  : option.label
              }
              checked={!hiddenKinds.has(option.id)}
              icon={option.icon}
              onClick={() => toggleKind(option.id)}
            />
          ))}
        </>
      ) : null}

      {source === "linear" && linearTeams.length > 0 ? (
        <>
          <SectionLabel>Teams</SectionLabel>
          {linearTeams.map((team) => (
            <FilterItem
              key={team.id}
              label={team.name || team.key}
              checked={!hiddenTeams.has(team.id)}
              onClick={() => toggleLinearTeam(team.id)}
            />
          ))}
        </>
      ) : null}

      {source === "linear" && linearProjects.length > 0 ? (
        <>
          <SectionLabel>Projects</SectionLabel>
          {linearProjects.map((project) => (
            <FilterItem
              key={project.id}
              label={project.name}
              checked={!hiddenLinearProjects.has(project.id)}
              onClick={() => toggleLinearProject(project.id)}
            />
          ))}
        </>
      ) : null}

      {source !== "linear" && projects.length > 0 ? (
        <>
          <SectionLabel>Projects</SectionLabel>
          {projects.map((project) => (
            <FilterItem
              key={project.path}
              label={project.name}
              checked={!hiddenProjects.has(project.path)}
              icon={
                project.logoPath ? (
                  <ProjectLogoIcon
                    path={project.logoPath}
                    className="size-3.5 shrink-0 rounded-sm"
                    imageClassName="size-3.5"
                  />
                ) : undefined
              }
              onClick={() => toggleProject(project.path)}
            />
          ))}
        </>
      ) : null}

      {hasActiveInboxFilters(filters, source, hiddenLinearTeamIds) ? (
        <>
          <div role="separator" className="my-1 h-px bg-content/10" />
          <button
            type="button"
            role="menuitem"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              onChange(DEFAULT_INBOX_FILTERS);
              if (teamsActive) onLinearTeamsChange([]);
            }}
            className="flex h-7 w-full items-center rounded-lg px-2 text-left text-[13px] leading-none text-content/70 hover:bg-content/5 hover:text-content"
          >
            Clear filters
          </button>
        </>
      ) : null}
    </Popover>
  );
}

function SectionLabel({ children }: { children: string }) {
  return (
    <div className="px-2 pb-0.5 pt-2 text-[10px] font-semibold uppercase tracking-[0.08em] text-content/40">
      {children}
    </div>
  );
}

function FilterItem({
  label,
  checked,
  icon,
  onClick,
}: {
  label: string;
  checked: boolean;
  icon?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
      className="flex h-7 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] leading-none text-content hover:bg-content/5"
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {checked ? (
        <Check className="size-3.5 shrink-0" strokeWidth={2.25} />
      ) : null}
    </button>
  );
}
