import { Bot, Check, CircleX, File, MessageSquare, PenLine, Search, Terminal, Wrench } from "../chrome/icons"
import { HarnessIcon } from "../chrome/HarnessIcon"
import { Shimmer } from "./Shimmer"
import type { FilePaneTab } from "../lib/layout"
import type { Session, SubagentStep } from "../lib/session"
import { modelDisplayName, subagentStatusOf, type SubagentStatus } from "../lib/subagents"

type Props = {
  file: FilePaneTab
  sessions: Session[]
}

export const SubagentSurface = ({ file, sessions }: Props) => {
  const source = file.subagent
  const session = source ? sessions.find((entry) => entry.id === source.sessionId) : undefined
  const block = source ? session?.blocks.find((entry) => entry.id === source.blockId) : undefined

  if (!source || !session || !block) {
    return (
      <div className="grid h-full place-items-center p-6 text-center">
        <p className="text-[13px] text-content/70">This subagent is no longer in the session.</p>
      </div>
    )
  }

  const status = subagentStatusOf(block)
  const running = status === "running"
  const title = block.tool?.title || source.title || "Subagent"
  const model = modelDisplayName(session.harness, block.tool?.model)
  const steps = block.tool?.steps ?? []
  const result = !running && block.tool?.detail ? block.tool.detail : undefined

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto overscroll-none">
      <header className="flex items-start gap-3 border-b border-content/10 px-5 py-4">
        <StatusMark status={status} className="mt-1 size-4" />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          {running ? (
            <Shimmer className="min-w-0 truncate text-[15px] font-medium leading-snug" duration={1.6}>
              {title}
            </Shimmer>
          ) : (
            <h1 className="truncate text-[15px] font-medium leading-snug">{title}</h1>
          )}
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[12px] text-content/50">
            <span className="flex items-center gap-1">
              <HarnessIcon harness={session.harness} className="size-3" />
              {model ?? "Model not reported"}
            </span>
            <span aria-hidden>·</span>
            <span>{statusLabel(status)}</span>
            {steps.length > 0 ? (
              <>
                <span aria-hidden>·</span>
                <span className="tabular-nums">
                  {steps.length} {steps.length === 1 ? "step" : "steps"}
                </span>
              </>
            ) : null}
          </div>
        </div>
      </header>
      <div className="flex flex-col gap-4 px-5 py-4">
        {steps.length === 0 ? (
          <p className="text-[13px] text-content/45">
            {running ? "Starting up…" : "No activity was recorded for this subagent."}
          </p>
        ) : (
          <StepList steps={steps} running={running} />
        )}
        {result ? (
          <section className="flex flex-col gap-1.5">
            <h2 className="text-[11px] font-medium uppercase tracking-wide text-content/40">
              {status === "failed" ? "Error" : "Result"}
            </h2>
            <pre className="whitespace-pre-wrap break-words rounded-md bg-content/5 px-3 py-2 font-sans text-[13px] leading-relaxed text-content/80">
              {result}
            </pre>
          </section>
        ) : null}
      </div>
    </div>
  )
}

const statusLabel = (status: SubagentStatus): string =>
  status === "running" ? "Working" : status === "failed" ? "Failed" : "Done"

export const StepList = ({ steps, running }: { steps: SubagentStep[]; running: boolean }) => {
  const last = steps.length - 1
  return (
    <ol className="flex flex-col gap-0.5">
      {steps.map((step, index) => {
        const live = running && index === last
        return (
          <li key={step.id} title={step.title} className="flex min-w-0 items-start gap-2 py-0.5">
            <StepIcon kind={step.kind} />
            {live ? (
              <Shimmer className="min-w-0 truncate text-[13px] leading-snug" duration={1.6}>
                {step.title}
              </Shimmer>
            ) : (
              <span
                className={`min-w-0 truncate text-[13px] leading-snug ${
                  step.kind === "message" ? "italic text-content/55" : "text-content/70"
                }`}
              >
                {step.title}
              </span>
            )}
          </li>
        )
      })}
    </ol>
  )
}

const StepIcon = ({ kind }: { kind?: string }) => {
  const props = { className: "mt-[3px] size-3.5 shrink-0 text-content/40", strokeWidth: 1.75 }
  switch ((kind ?? "").toLowerCase()) {
    case "edit":
    case "write":
      return <PenLine {...props} />
    case "read":
      return <File {...props} />
    case "search":
    case "fetch":
      return <Search {...props} />
    case "execute":
    case "shell":
    case "bash":
      return <Terminal {...props} />
    case "agent":
      return <Bot {...props} />
    case "message":
      return <MessageSquare {...props} />
    default:
      return <Wrench {...props} />
  }
}

export const StatusMark = ({ status, className = "mt-0.5 size-3.5" }: { status: SubagentStatus; className?: string }) => {
  if (status === "running") {
    return <Bot className={`${className} shrink-0 text-content/45`} strokeWidth={1.75} />
  }
  if (status === "failed") {
    return <CircleX className={`${className} shrink-0 text-red-400`} strokeWidth={1.75} />
  }
  return <Check className={`${className} shrink-0 text-emerald-400`} strokeWidth={1.75} />
}
