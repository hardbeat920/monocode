import { modelsFor } from "./models"
import {
  sessionDisplayTitle,
  type Block,
  type HarnessId,
  type Session,
  type SubagentStep,
} from "./session"

export type SubagentStatus = "running" | "done" | "failed"

export type Subagent = {
  id: string
  sessionId: string
  sessionTitle: string
  harness: HarnessId
  title: string
  detail?: string
  /** Display name of the model the subagent runs under, when reported. */
  model?: string
  steps: SubagentStep[]
  status: SubagentStatus
}

export type SubagentGroup = {
  sessionId: string
  sessionTitle: string
  harness: HarnessId
  agents: Subagent[]
}

const RUNNING = new Set(["in_progress", "pending", "running"])
const FAILED = new Set(["failed", "error", "errored", "cancelled", "canceled", "killed"])

const isAgentBlock = (block: Block): boolean =>
  block.role === "tool" && (block.tool?.kind ?? "").toLowerCase() === "agent"

export const subagentStatusOf = (block: Block): SubagentStatus => {
  const status = (block.tool?.status ?? "").toLowerCase()
  if (block.streaming || RUNNING.has(status)) return "running"
  if (FAILED.has(status)) return "failed"
  return "done"
}

const currentTurnBlocks = (blocks: Block[]): Block[] => {
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].role === "user") return blocks.slice(i + 1)
  }
  return blocks
}

/** Subagents spawned in each session's latest turn, running ones first. */
export const subagentsFromSessions = (sessions: Session[]): SubagentGroup[] =>
  sessions
    .filter((session) => !session.inboxAsk)
    .flatMap((session) => {
      const sessionTitle = sessionDisplayTitle(session.title, session.harness)
      const agents = currentTurnBlocks(session.blocks)
        .filter(isAgentBlock)
        .map((block): Subagent => ({
          id: block.id,
          sessionId: session.id,
          sessionTitle,
          harness: session.harness,
          title: block.tool?.title || block.text || "Subagent",
          detail: block.tool?.detail || undefined,
          model: modelDisplayName(session.harness, block.tool?.model),
          steps: block.tool?.steps ?? [],
          status: subagentStatusOf(block),
        }))
        .sort((a, b) => rank(a.status) - rank(b.status))
      if (agents.length === 0) return []
      return [{ sessionId: session.id, sessionTitle, harness: session.harness, agents }]
    })
    .sort((a, b) => runningCount(b) - runningCount(a))

export const modelDisplayName = (
  harness: HarnessId,
  nativeId: string | undefined,
): string | undefined => {
  if (!nativeId) return undefined
  const models = modelsFor(harness)
  const match =
    models.find((model) => model.nativeId === nativeId || model.id === nativeId) ??
    models.find((model) => model.nativeId && nativeId.startsWith(`${model.nativeId}-`))
  return match?.name ?? prettyModelId(nativeId)
}

/** `claude-opus-4-8` → `Opus 4.8`, `claude-haiku-4-5-20251001` → `Haiku 4.5`. */
export const prettyModelId = (id: string): string => {
  const parts = id
    .replace(/\[.*$/, "")
    .split(/[-_]/)
    .filter((part) => part && part !== "claude" && !/^\d{8}$/.test(part))
  if (parts.length === 0) return id
  const words: string[] = []
  const digits: string[] = []
  for (const part of parts) {
    if (/^\d+$/.test(part)) digits.push(part)
    else words.push(part[0].toUpperCase() + part.slice(1))
  }
  return [words.join(" "), digits.join(".")].filter(Boolean).join(" ")
}

export const countRunningSubagents = (groups: SubagentGroup[]): number =>
  groups.reduce((sum, group) => sum + runningCount(group), 0)

const runningCount = (group: SubagentGroup): number =>
  group.agents.filter((agent) => agent.status === "running").length

const rank = (status: SubagentStatus): number =>
  status === "running" ? 0 : status === "failed" ? 1 : 2
