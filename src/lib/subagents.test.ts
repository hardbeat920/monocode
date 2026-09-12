import { describe, expect, it } from "vitest"
import { countRunningSubagents, modelDisplayName, prettyModelId, subagentsFromSessions } from "./subagents"
import { newSession, type Block, type Session } from "./session"

const chat = (blocks: Block[], patch: Partial<Session> = {}): Session => {
  const session = newSession("claude", "/tmp/a")
  session.title = "claude · Fix the sidebar"
  return { ...session, ...patch, blocks }
}

const agent = (id: string, status = "in_progress", detail?: string): Block => ({
  id,
  role: "tool",
  text: `Explore ${id}`,
  tool: { kind: "agent", title: `Explore ${id}`, status, detail },
})

const user = (id: string): Block => ({ id, role: "user", text: "go" })

describe("subagentsFromSessions", () => {
  it("skips sessions without agent tools", () => {
    const edit: Block = { id: "t", role: "tool", text: "Edit", tool: { kind: "edit" } }
    expect(subagentsFromSessions([chat([user("u"), edit])])).toEqual([])
  })

  it("only reports agents from the latest turn, running first", () => {
    const session = chat([
      user("u1"),
      agent("old", "completed"),
      user("u2"),
      agent("done", "completed"),
      agent("live", "in_progress", "code reviewer subagent"),
      agent("bad", "failed"),
    ])
    const groups = subagentsFromSessions([session])
    expect(groups).toHaveLength(1)
    expect(groups[0].sessionTitle).toBe("Fix the sidebar")
    expect(groups[0].agents.map((a) => [a.id, a.status])).toEqual([
      ["live", "running"],
      ["bad", "failed"],
      ["done", "done"],
    ])
    expect(groups[0].agents[0].detail).toBe("code reviewer subagent")
    expect(countRunningSubagents(groups)).toBe(1)
  })

  it("puts sessions with running agents ahead of finished ones", () => {
    const idle = chat([user("u"), agent("x", "completed")], { id: "idle" })
    const busy = chat([user("u"), agent("y")], { id: "busy" })
    const groups = subagentsFromSessions([idle, busy])
    expect(groups.map((g) => g.sessionId)).toEqual(["busy", "idle"])
  })

  it("carries model name and steps", () => {
    const block: Block = {
      id: "a",
      role: "tool",
      text: "Explore",
      tool: {
        kind: "agent",
        title: "Explore",
        status: "in_progress",
        model: "claude-haiku-4-5-20251001",
        steps: [{ id: "s1", title: "Grep foo", kind: "search" }],
      },
    }
    const [group] = subagentsFromSessions([chat([user("u"), block])])
    expect(group.agents[0].model).toBe("Haiku 4.5")
    expect(group.agents[0].steps).toEqual([{ id: "s1", title: "Grep foo", kind: "search" }])
  })

  it("prettifies unknown model ids", () => {
    expect(modelDisplayName("claude", "claude-opus-4-8")).toBe("Opus 4.8")
    expect(prettyModelId("claude-haiku-4-5-20251001")).toBe("Haiku 4.5")
    expect(prettyModelId("gpt-5")).toBe("Gpt 5")
    expect(modelDisplayName("claude", undefined)).toBeUndefined()
  })

  it("ignores inbox ask sessions", () => {
    const session = chat([user("u"), agent("y")], { inboxAsk: { id: "ask" } as never })
    expect(subagentsFromSessions([session])).toEqual([])
  })
})
