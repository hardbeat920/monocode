import { Bot, X } from "./icons"
import { HarnessIcon } from "./HarnessIcon"
import { IconButton } from "./TitleBar"
import { Shimmer } from "../surfaces/Shimmer"
import { StatusMark } from "../surfaces/SubagentSurface"
import type { Subagent, SubagentGroup } from "../lib/subagents"

type Props = {
  groups: SubagentGroup[]
  activeSessionId?: string
  onSelectSession: (sessionId: string) => void
  onOpenSubagent: (sessionId: string, blockId: string) => void
  onClose: () => void
}

export const AgentsPanel = ({
  groups,
  activeSessionId,
  onSelectSession,
  onOpenSubagent,
  onClose,
}: Props) => {
  const running = groups.reduce(
    (sum, group) => sum + group.agents.filter((agent) => agent.status === "running").length,
    0,
  )
  return (
    <aside
      aria-label="Agents"
      className="sidebar-glass flex h-full w-72 min-h-0 shrink-0 flex-col border-r border-content/10"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center gap-1 border-b border-content/10 pl-3 pr-1.5"
        data-tauri-drag-region="deep"
      >
        <span className="min-w-0 flex-1 truncate text-sm font-medium leading-tight">
          Agents
        </span>
        {running > 0 ? (
          <span className="mr-1 text-[11px] tabular-nums text-content/40">{running}</span>
        ) : null}
        <IconButton label="Close agents" onClick={onClose}>
          <X className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto overscroll-none p-2">
        {groups.length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-3 py-8 text-center text-xs text-content/40">
            <Bot className="size-5" strokeWidth={1.5} />
            No subagents working
          </div>
        ) : (
          groups.map((group) => (
            <section key={group.sessionId} className="flex flex-col gap-px">
              <button
                type="button"
                onClick={() => onSelectSession(group.sessionId)}
                aria-current={group.sessionId === activeSessionId ? "true" : undefined}
                className={`flex min-w-0 items-center gap-1.5 rounded-md px-2 py-1 text-left text-[11px] font-medium text-content/60 ${
                  group.sessionId === activeSessionId ? "bg-content/10 text-content" : "hover:bg-content/5"
                }`}
              >
                <HarnessIcon harness={group.harness} className="size-3 shrink-0" />
                <span className="min-w-0 flex-1 truncate">{group.sessionTitle}</span>
              </button>
              {group.agents.map((agent) => (
                <SubagentRow
                  key={agent.id}
                  agent={agent}
                  onOpen={() => onOpenSubagent(agent.sessionId, agent.id)}
                />
              ))}
            </section>
          ))
        )}
      </div>
    </aside>
  )
}

const SubagentRow = ({ agent, onOpen }: { agent: Subagent; onOpen: () => void }) => {
  const label = [agent.title, agent.model, agent.detail, agent.status].filter(Boolean).join(", ")
  const running = agent.status === "running"
  const subtitle = [agent.model, agent.detail].filter(Boolean).join(" · ")
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onOpen}
      className="flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-1.5 text-left hover:bg-content/8"
    >
      <StatusMark status={agent.status} />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        {running ? (
          <Shimmer className="min-w-0 truncate text-[13px] leading-snug" duration={1.6}>
            {agent.title}
          </Shimmer>
        ) : (
          <span className="truncate text-[13px] leading-snug text-content/70">{agent.title}</span>
        )}
        {subtitle ? (
          <span className="truncate text-[11px] leading-tight text-content/50">{subtitle}</span>
        ) : null}
      </span>
    </button>
  )
}
