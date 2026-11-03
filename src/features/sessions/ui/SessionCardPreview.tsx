import { useEffect, type ReactNode } from "react";
import { formatInteger } from "../../../shared/lib/numbers";
import { prettyCwd } from "../../../shared/lib/paths";
import { Popover } from "../../../shared/ui/Popover";
import { NO_BRANCH_LABEL } from "../../source-control/model/worktrees";
import type { SessionSummary } from "../data/sessionStore";
import { resolveModel } from "../model/models";
import { sessionDisplayTitle } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";

export function SessionCardPreview({
  anchor,
  id,
  session,
  status,
  onClose,
  onPointerEnter,
  onPointerLeave,
}: {
  anchor: HTMLElement;
  id: string;
  session: SessionSummary;
  status: ReactNode;
  onClose: () => void;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
}) {
  useEffect(() => {
    const onScroll = (event: Event) => {
      // Transcript prefetching can scroll a different pane while hovering.
      // Only scrolling the card's own ancestors should dismiss its preview.
      if (
        event.target === document ||
        (event.target instanceof Element && event.target.contains(anchor))
      ) {
        onClose();
      }
    };
    const onOtherCard = (event: PointerEvent) => {
      const card =
        event.target instanceof Element
          ? event.target.closest<HTMLElement>("[data-session-card]")
          : null;
      if (card && card.dataset.sessionCard !== session.id) onClose();
    };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("pointerover", onOtherCard, true);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("pointerover", onOtherCard, true);
    };
  }, [anchor, onClose, session.id]);

  const additions = session.additions ?? 0;
  const deletions = session.deletions ?? 0;

  return (
    <Popover
      anchor={anchor}
      side="right"
      align="start"
      gap={8}
      width={260}
      maxHeight={440}
      role="tooltip"
      id={id}
      data-session-preview={session.id}
      onDismiss={onClose}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      className="overflow-y-auto overscroll-contain px-3 py-2.5"
    >
      <div className="flex items-center justify-between gap-3">
        <span className="flex min-w-0 items-center gap-1.5 text-[11px] text-content/55">
          <HarnessIcon
            harness={session.harness}
            className="size-3.5 shrink-0"
          />
          <span className="min-w-0 truncate">
            {resolveModel(session.harness, session.model).name}
          </span>
        </span>
        {status}
      </div>
      <p className="mt-1.5 break-words text-[13px] font-semibold leading-5 text-content">
        {sessionDisplayTitle(session.title, session.harness)}
      </p>
      <dl className="mt-2 grid grid-cols-[4.5rem_minmax(0,1fr)] gap-x-2 gap-y-1.5 border-t border-stroke pt-2">
        {session.branch || session.worktreeRemoved ? (
          <Detail label="Branch">
            {session.worktreeRemoved ? NO_BRANCH_LABEL : session.branch}
          </Detail>
        ) : null}
        <Detail
          label={
            session.worktreeCwd || session.worktreeRemoved
              ? "Worktree"
              : "Directory"
          }
        >
          {session.worktreeRemoved ? (
            <>
              <span className="text-content/50">Removed</span>
              {session.worktreeCwd ? (
                <span className="mt-0.5 block text-content/40">
                  {prettyCwd(session.worktreeCwd)}
                </span>
              ) : null}
            </>
          ) : (
            prettyCwd(session.worktreeCwd || session.cwd)
          )}
        </Detail>
        {additions > 0 || deletions > 0 ? (
          <Detail label="Changes">
            <span className="flex gap-2 font-medium tabular-nums">
              {additions > 0 ? (
                <span className="text-diff-add-fg">
                  +{formatInteger(additions)}
                </span>
              ) : null}
              {deletions > 0 ? (
                <span className="text-diff-del-fg">
                  −{formatInteger(deletions)}
                </span>
              ) : null}
            </span>
          </Detail>
        ) : null}
        {session.linkedWorkItem ? (
          <Detail
            label={
              session.linkedWorkItem.kind === "pr" ? "Pull request" : "Issue"
            }
          >
            {session.linkedWorkItem.repo} #{session.linkedWorkItem.number}
          </Detail>
        ) : null}
      </dl>
    </Popover>
  );
}

function Detail({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-[11px] leading-4 text-content/45">{label}</dt>
      <dd className="min-w-0 text-[11px] leading-4 text-content/80 [overflow-wrap:anywhere]">
        {children}
      </dd>
    </>
  );
}
