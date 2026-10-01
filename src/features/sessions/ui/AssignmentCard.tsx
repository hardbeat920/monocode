import type { ReactNode } from "react";
import type { AssignmentReceipt } from "../../agent-app/model/assignments";
import type { SessionUpdateStatus } from "../../agent-app/model/sessionLinks";
import { HARNESS_TITLE, HARNESSES, type HarnessId } from "../model/session";
import {
  AGENT_STATUS,
  AgentRow,
  IdsLine,
  OpenSessionAction,
  RowActions,
  StatusWord,
  useReadableModel,
} from "./AgentRow";

/** A task the operator handed to another session, as that agent's row. */
export function AssignmentCard({
  receipt,
  outcome,
  working,
  childTitle,
  onOpenSession,
}: {
  receipt: AssignmentReceipt;
  /** How the child's turn for this task ended, once it has reported back. */
  outcome?: SessionUpdateStatus;
  /** The child is working on this task: how long it has been at it. */
  working?: ReactNode;
  childTitle?: string;
  onOpenSession?: (id: string) => void;
}) {
  const name = receipt.name ?? childTitle ?? receipt.childId;
  const model = useReadableModel(receipt.harness, receipt.model);
  // The CLI keys a launch by the new session's own id; a send uses a fresh request id.
  const newSession = receipt.requestKey === receipt.childId;
  const harness = HARNESSES.includes(receipt.harness as HarnessId)
    ? HARNESS_TITLE[receipt.harness as HarnessId]
    : receipt.harness;
  const active = !outcome && working != null;
  const state = outcome
    ? AGENT_STATUS[outcome].word
    : active
      ? "working"
      : undefined;
  const status = outcome ? <StatusWord status={outcome} /> : working;
  return (
    <AgentRow
      harness={receipt.harness}
      name={name}
      model={model}
      active={active}
      status={
        newSession ? (
          status
        ) : (
          <>
            <span>follow-up{status ? " ·" : ""}</span>
            {status}
          </>
        )
      }
      label={[name, newSession ? undefined : "follow-up", state]
        .filter(Boolean)
        .join(", ")}
      subject="task"
    >
      <p className="whitespace-pre-wrap break-words text-sm leading-relaxed text-content/70">
        {receipt.task}
      </p>
      <IdsLine
        parts={[
          newSession
            ? "New session"
            : receipt.kind === "linked"
              ? `Request ${receipt.generation}`
              : "Untracked message",
          newSession && harness,
          receipt.model,
        ]}
        id={receipt.childId}
      />
      {onOpenSession ? (
        <RowActions>
          <OpenSessionAction
            label={`Open ${name}`}
            onOpen={() => onOpenSession(receipt.childId)}
          />
        </RowActions>
      ) : null}
    </AgentRow>
  );
}
