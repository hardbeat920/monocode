import { CircleAlert } from "./icons";
import { createPortal } from "react-dom";
import type { ApprovalDecision } from "../lib/harness";
import type { PendingApprovalNotice } from "../lib/approvalToast";
import { LAYER } from "../lib/layers";
import {
  sessionDisplayTitle,
  getHarnessTitle,
  type Session,
} from "../lib/session";
import { HarnessIcon } from "./HarnessIcon";
import { t } from "../lib/i18n";

type Notice = PendingApprovalNotice & { session: Session };

type Props = {
  notices: Notice[];
  onFocusSession: (sessionId: string) => void;
  onApproval: (
    sessionId: string,
    requestId: number,
    decision: ApprovalDecision,
  ) => void;
};

export function ApprovalToasts({ notices, onFocusSession, onApproval }: Props) {
  if (notices.length === 0) return null;

  return createPortal(
    <div
      aria-live="polite"
      style={{ zIndex: LAYER.toast }}
      className="pointer-events-none fixed right-3 top-3 flex w-[min(360px,calc(100vw-24px))] flex-col gap-2"
    >
      {notices.map((notice) => (
        <ApprovalToastCard
          key={`${notice.sessionId}:${notice.requestId}`}
          notice={notice}
          onFocusSession={onFocusSession}
          onApproval={onApproval}
        />
      ))}
    </div>,
    document.body,
  );
}

function ApprovalToastCard({
  notice,
  onFocusSession,
  onApproval,
}: {
  notice: Notice;
  onFocusSession: (sessionId: string) => void;
  onApproval: Props["onApproval"];
}) {
  const { session, label, requestId } = notice;
  const title = sessionDisplayTitle(session.title, session.harness);
  const harness = getHarnessTitle()[session.harness];

  const openSession = () => onFocusSession(session.id);

  return (
    <article
      className="approval-toast pointer-events-auto overflow-hidden rounded-xl border border-content/20 border-dashed bg-content/10 shadow-xl backdrop-blur-xl"
      role="status"
    >
      <button
        type="button"
        onClick={openSession}
        className="flex w-full flex-col gap-2 px-3.5 py-3 text-left hover:bg-content/5"
      >
        <span className="flex items-center gap-2">
          <HarnessIcon harness={session.harness} className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-snug text-content">
            {title}
          </span>
          <span className="flex shrink-0 items-center gap-1 text-[11px] text-amber-400">
            <CircleAlert className="size-3.5" strokeWidth={1.75} />
            <span>{notice.kind === "question" ? t("Question") : t("Approval")}</span>
          </span>
        </span>
        <span className="line-clamp-3 text-[12px] leading-relaxed text-content/70">
          {label}
        </span>
        <span className="text-[11px] text-content/40">{harness}</span>
      </button>
      {notice.kind === "question" ? null : (
        <div className="flex gap-2 border-t border-content/10 px-3.5 py-2.5">
          <button
            type="button"
            className="flex-1 rounded-md bg-content px-2.5 py-1 text-[11px] font-medium text-background-base hover:bg-content/80"
            onClick={() => onApproval(session.id, requestId, "allow")}
          >
            {t("Allow")}
          </button>
          <button
            type="button"
            className="flex-1 rounded-md bg-content/10 px-2.5 py-1 text-[11px] font-medium text-content/70 hover:bg-content/20"
            onClick={() => onApproval(session.id, requestId, "deny")}
          >
            {t("Deny")}
          </button>
        </div>
      )}
    </article>
  );
}
