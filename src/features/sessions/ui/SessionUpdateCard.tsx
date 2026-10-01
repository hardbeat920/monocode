import { useLayoutEffect, useRef, useState } from "react";
import type { SessionUpdateView } from "../../agent-app/model/sessionLinks";
import {
  AGENT_STATUS,
  AgentRow,
  CopyReplyAction,
  IdsLine,
  OpenSessionAction,
  RowActions,
  StatusWord,
  useReadableModel,
} from "./AgentRow";

/** Session updates as one agent row per child session, in place of the raw tags the agent reads. */
export function SessionUpdateCard({
  updates,
  onOpenSession,
}: {
  updates: SessionUpdateView[];
  onOpenSession?: (sessionId: string) => void;
}) {
  return (
    <div data-session-updates className="flex min-w-0 flex-col pb-1">
      {updates.map((update) => (
        <SessionUpdateRow
          key={`${update.childId}:${update.kind}:${update.generation}:${update.requestId ?? ""}`}
          update={update}
          onOpenSession={
            update.status === "removed" ? undefined : onOpenSession
          }
        />
      ))}
    </div>
  );
}

function SessionUpdateRow({
  update,
  onOpenSession,
}: {
  update: SessionUpdateView;
  onOpenSession?: (sessionId: string) => void;
}) {
  const name = update.assignment?.name ?? (update.title || update.childId);
  const model = useReadableModel(update.harness, update.model);
  const open = onOpenSession
    ? () => onOpenSession(update.childId)
    : undefined;
  const label = `${name} ${AGENT_STATUS[update.status].word}`;
  const ids = (
    <IdsLine
      parts={[
        update.status === "stopped" && !!update.reply && "Partial reply",
        `Request ${update.generation}`,
        update.repeat && "Sent again after a failed delivery",
        update.model,
      ]}
      id={update.childId}
    />
  );

  if (update.kind === "blocked")
    return (
      <AgentRow
        harness={update.harness}
        name={name}
        model={model}
        status={<StatusWord status={update.status} />}
        label={label}
        subject="details"
        below={
          <div className="text-content/70">
            {update.label ? (
              <p className="whitespace-pre-wrap break-words">{update.label}</p>
            ) : null}
            {open ? (
              <RowActions>
                <OpenSessionAction
                  warn
                  label={`${update.status === "question" ? "Answer" : "Review"} in ${name}`}
                  onOpen={open}
                />
              </RowActions>
            ) : null}
          </div>
        }
      />
    );

  return (
    <AgentRow
      harness={update.harness}
      name={name}
      model={model}
      status={<StatusWord status={update.status} />}
      label={label}
      subject={update.reply ? "reply" : "details"}
      below={
        update.status === "failed" && update.error ? (
          <p className="whitespace-pre-wrap break-words text-red-400">
            {update.error}
          </p>
        ) : undefined
      }
    >
      {update.reply ? (
        <UpdateReply text={update.reply} truncated={update.truncated} />
      ) : null}
      {update.status === "stopped" ? ids : null}
      {open || update.reply ? (
        <RowActions>
          {open ? (
            <OpenSessionAction label={`Open ${name}`} onOpen={open} />
          ) : null}
          {update.reply ? (
            <CopyReplyAction
              text={update.reply}
              truncated={update.truncated}
            />
          ) : null}
        </RowActions>
      ) : null}
      {update.status === "stopped" ? null : ids}
    </AgentRow>
  );
}

function UpdateReply({
  text,
  truncated,
}: {
  text: string;
  truncated: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const ref = useRef<HTMLParagraphElement>(null);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || expanded) return;
    const measure = () =>
      setOverflows(element.scrollHeight > element.clientHeight + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [text, expanded]);

  return (
    <>
      <p
        ref={ref}
        className={`whitespace-pre-wrap break-words text-sm leading-relaxed text-content/80 ${expanded ? "" : "line-clamp-4"}`}
      >
        {truncated ? "…" : ""}
        {text}
      </p>
      {overflows || expanded ? (
        <button
          type="button"
          aria-expanded={expanded}
          className="mt-1 rounded px-1 py-0.5 text-xs text-content/60 hover:bg-content/8 hover:text-content"
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "Show less" : "Show more"}
        </button>
      ) : null}
    </>
  );
}
