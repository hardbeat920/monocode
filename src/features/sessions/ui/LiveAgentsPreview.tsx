import { useEffect, useId, useRef, useState } from "react";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { formatLiveElapsed, type LiveAgent } from "../model/liveAgents";
import { formatTokens } from "../model/contextUsage";
import { projectKey, projectName } from "../../../shared/lib/paths";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLabel,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";
import { Check, ChevronDown, ChevronUp, CircleAlert } from "../../../shared/ui/icons";
import { HarnessIcon } from "./HarnessIcon";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import { TerminalSpinner } from "./TerminalSpinner";
import { Popover } from "../../../shared/ui/Popover";

const LIVE_AGENT_MIN = 2;
const LIVE_AGENT_CAP = 4;

type Props = {
  agents: LiveAgent[];
  activeSessionId?: string;
  onSelect?: (sessionId: string) => void;
  bottomSpacing?: boolean;
  groupLabels?: Record<string, string>;
  groupColors?: Record<string, number>;
  groupCustomColors?: Record<string, string>;
  groupMascots?: Record<string, string>;
};

export function LiveAgentsPreview({
  agents,
  activeSessionId,
  onSelect,
  bottomSpacing = false,
  groupLabels: groupLabelsProp,
  groupColors: groupColorsProp,
  groupCustomColors: groupCustomColorsProp,
  groupMascots: groupMascotsProp,
}: Props) {
  const [loadedGroupLabels] = useState(loadTabGroupLabels);
  const [loadedGroupColors] = useState(loadTabGroupColors);
  const [loadedGroupCustomColors] = useState(loadTabGroupCustomColors);
  const [loadedGroupMascots] = useState(loadTabGroupMascots);
  const groupLabels = groupLabelsProp ?? loadedGroupLabels;
  const groupColors = groupColorsProp ?? loadedGroupColors;
  const groupCustomColors = groupCustomColorsProp ?? loadedGroupCustomColors;
  const groupMascots = groupMascotsProp ?? loadedGroupMascots;
  const [expanded, setExpanded] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const lockList = useLockOverscroll<HTMLDivElement>();
  const ticking =
    agents.length >= LIVE_AGENT_MIN &&
    agents.some((agent) => !agent.done && agent.startedAt != null);

  useEffect(() => {
    if (!ticking) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [ticking]);

  if (agents.length < LIVE_AGENT_MIN) return null;

  const extra = agents.length - LIVE_AGENT_CAP;
  const visible =
    expanded || extra <= 0 ? agents : agents.slice(0, LIVE_AGENT_CAP);

  return (
    <section
      aria-label="Working agents"
      className={`shrink-0 px-2 ${bottomSpacing ? "pb-2" : ""}`}
      data-live-agents-preview="full"
    >
      <span className="sr-only" aria-live="polite" aria-atomic="true">
        {agents.length} working agents
      </span>
      <div className="overflow-hidden rounded-lg bg-content/5">
        <div className="flex items-center gap-2 px-3.5 py-1.5">
          <span
            aria-hidden
            className="size-1.5 shrink-0 rounded-full bg-accent shadow-[0_0_8px_var(--color-accent)] motion-safe:animate-pulse"
          />
          <span className="min-w-0 flex-1 truncate text-xs text-content/50">
            Working
          </span>
          <span className="text-[11px] tabular-nums text-content/40">
            {agents.length}
          </span>
        </div>
        <div
          ref={expanded ? lockList : undefined}
          className={`flex flex-col gap-px px-1 ${
            extra > 0 ? "" : "pb-1"
          } ${expanded ? "max-h-[45vh] overflow-y-auto overscroll-none" : ""}`}
        >
          {visible.map((agent) => (
            <LiveAgentCard
              key={agent.id}
              agent={agent}
              now={now}
              selected={agent.id === activeSessionId}
              onSelect={onSelect}
              groupLabels={groupLabels}
              groupColors={groupColors}
              groupCustomColors={groupCustomColors}
              groupMascots={groupMascots}
            />
          ))}
        </div>
        {extra > 0 ? (
          <button
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded((open) => !open)}
            className="flex w-full items-center justify-center gap-1 px-2 py-1.5 text-[11px] text-content/50 hover:bg-content/8 hover:text-content"
          >
            {expanded ? (
              <ChevronUp className="size-3" strokeWidth={1.75} />
            ) : (
              <ChevronDown className="size-3" strokeWidth={1.75} />
            )}
            {expanded ? "Show less" : `${extra} more`}
          </button>
        ) : null}
      </div>
    </section>
  );
}

function LiveAgentCard({
  agent,
  now,
  selected,
  onSelect,
  groupLabels,
  groupColors,
  groupCustomColors,
  groupMascots,
}: {
  agent: LiveAgent;
  now: number;
  selected: boolean;
  onSelect?: (sessionId: string) => void;
  groupLabels: Record<string, string>;
  groupColors: Record<string, number>;
  groupCustomColors: Record<string, string>;
  groupMascots: Record<string, string>;
}) {
  const root = useRef<HTMLButtonElement>(null);
  const cacheTooltipId = useId();
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const seed = projectName(agent.cwd);
  const key = projectKey(agent.cwd);
  const project = resolveTabGroupLabel(key, groupLabels, seed);
  const color = resolveTabGroupColor(key, groupColors, groupCustomColors, seed);
  const elapsed = agent.done
    ? agent.durationMs != null
      ? formatLiveElapsed(0, agent.durationMs)
      : ""
    : agent.startedAt != null
      ? formatLiveElapsed(agent.startedAt, now)
      : "";
  const activity = agent.needsApproval
    ? "Need approval"
    : agent.done
      ? "Done"
      : agent.activity;
  const live = !agent.needsApproval && !agent.done;
  const tokenCounts = [
    agent.turnMetrics?.inputTokens != null
      ? `${formatTokens(agent.turnMetrics.inputTokens)} input`
      : null,
    agent.turnMetrics?.outputTokens != null
      ? `${formatTokens(agent.turnMetrics.outputTokens)} output`
      : null,
  ].filter(Boolean);
  const cacheCounts = [
    agent.turnMetrics?.cacheReadTokens != null
      ? `${formatTokens(agent.turnMetrics.cacheReadTokens)} cached`
      : null,
    agent.turnMetrics?.cacheWriteTokens != null
      ? `${formatTokens(agent.turnMetrics.cacheWriteTokens)} cache written`
      : null,
    agent.turnMetrics?.cacheHitPercent != null
      ? `Cache hit ${Math.round(agent.turnMetrics.cacheHitPercent)}%`
      : null,
  ].filter(Boolean);
  const metricsLabel = tokenCounts.join(" · ");
  const showCacheTooltip = cacheCounts.length > 0 && (hovered || focused);
  const title = [agent.title, project, activity, elapsed, metricsLabel]
    .filter(Boolean)
    .join("\n");

  return (
    <button
      type="button"
      title={title}
      aria-label={[agent.title, project, activity, elapsed, metricsLabel]
        .filter(Boolean)
        .join(", ")}
      aria-current={selected ? "true" : undefined}
      aria-describedby={showCacheTooltip ? cacheTooltipId : undefined}
      data-live-agent-card={agent.id}
      ref={root}
      onClick={() => onSelect?.(agent.id)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      className={`relative flex w-full flex-col rounded-md px-2 py-1.5 text-left ${
        selected ? "bg-selection" : "hover:bg-content/8"
      }`}
    >
      <span className="flex min-w-0 items-center gap-2">
        <ProjectMascot
          project={seed}
          color={color}
          name={resolveTabGroupMascot(key, groupMascots)}
          className="size-2 shrink-0"
          active={live}
        />
        {live ? (
          <p className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-snug">
            {agent.title}
          </p>
        ) : (
          <span className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-snug">
            {agent.title}
          </span>
        )}
      </span>
      <span
        className={`mt-1 flex min-w-0 items-center gap-1.5 pl-4 text-[11px] leading-tight ${
          agent.needsApproval
            ? "text-amber-400"
            : agent.done
              ? "text-emerald-400"
              : "text-content/50"
        }`}
      >
        {agent.needsApproval ? (
          <CircleAlert className="size-3 shrink-0" strokeWidth={1.75} />
        ) : agent.done ? (
          <Check className="size-3 shrink-0" strokeWidth={2.25} />
        ) : (
          <TerminalSpinner className="inline-block w-3 select-none text-center text-[11px] leading-none" />
        )}
        <span className="min-w-0 truncate">{activity}</span>
      </span>
      <span className="mt-1 flex min-w-0 items-center gap-1.5 pl-4 text-[11px] leading-tight text-content/45">
        <HarnessIcon harness={agent.harness} className="size-3 shrink-0" />
        <span className="min-w-0 flex-1 truncate">{project}</span>
        {elapsed ? (
          <span className="shrink-0 tabular-nums">{elapsed}</span>
        ) : null}
      </span>
      {tokenCounts.length ? (
        <span className="mt-1 min-w-0 truncate pl-4 text-[10px] leading-tight text-content/40">
          {tokenCounts.join(" · ")}
        </span>
      ) : null}
      {showCacheTooltip ? (
        <Popover
          anchor={root}
          side="top"
          align="start"
          id={cacheTooltipId}
          role="tooltip"
          className="pointer-events-none w-max max-w-[min(240px,90vw)] px-2.5 py-1.5 text-[11px] leading-4 text-content"
        >
          {cacheCounts.join(" · ")}
        </Popover>
      ) : null}
    </button>
  );
}
