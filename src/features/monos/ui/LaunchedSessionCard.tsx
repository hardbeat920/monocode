import type { ReactNode } from "react";
import { projectName } from "../../../shared/lib/paths";
import {
  Chatting,
  CheckCircle,
  CircleAlert,
  CircleDashed,
  CircleX,
  Clock,
  Loader,
} from "../../../shared/ui/icons";
import {
  sessionDisplayTitle,
  type MonoSpawnedSession,
} from "../../sessions/model/session";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import type {
  LaunchedSessionState,
  LaunchedSessionView,
} from "../model/monoSpawnedSessions";

const STATUS: Record<LaunchedSessionState, { label: string; icon: ReactNode }> =
  {
    working: {
      label: "Working",
      icon: (
        <Loader
          className="size-4 shrink-0 animate-spin text-content/55 motion-reduce:animate-none"
          strokeWidth={1.75}
        />
      ),
    },
    input: {
      label: "Needs input",
      icon: (
        <Clock className="size-4 shrink-0 text-amber-400" strokeWidth={1.75} />
      ),
    },
    draft: {
      label: "Draft",
      icon: (
        <CircleDashed
          className="size-4 shrink-0 text-content/45"
          strokeWidth={1.75}
        />
      ),
    },
    failed: {
      label: "Failed",
      icon: (
        <CircleAlert
          className="size-4 shrink-0 text-red-400"
          strokeWidth={1.75}
        />
      ),
    },
    stopped: {
      label: "Stopped",
      icon: (
        <CircleX
          className="size-4 shrink-0 text-content/45"
          strokeWidth={1.75}
        />
      ),
    },
    done: {
      label: "Done",
      icon: (
        <CheckCircle
          className="size-4 shrink-0 text-emerald-400/90"
          strokeWidth={1.75}
        />
      ),
    },
  };

/**
 * A session the Mono started, as a line under its reply like an artifact:
 * a spinner while it works, a check once it is done, and the project it runs
 * in. Without `view` (a window that does not track sessions) it shows no
 * status.
 */
export function LaunchedSessionCard({
  launch,
  view,
  open = false,
  onOpen,
}: {
  launch: MonoSpawnedSession;
  view?: LaunchedSessionView;
  /** Shown in the sidebar right now. */
  open?: boolean;
  onOpen?: () => void;
}) {
  const title = sessionDisplayTitle(
    view?.title ?? launch.title,
    launch.harness,
  );
  const status = view ? STATUS[view.state] : undefined;
  return (
    <button
      type="button"
      data-launched-session={launch.sessionId}
      data-state={view?.state}
      aria-label={`Open session ${title}${status ? `, ${status.label.toLowerCase()}` : ""}`}
      aria-pressed={open}
      title={status?.label}
      onClick={onOpen}
      disabled={!onOpen}
      className={`flex w-fit max-w-[min(24rem,100%)] min-w-0 items-center gap-2.5 rounded-xl border py-1.5 pr-1.5 pl-3 text-left font-sans transition-colors hover:border-content/20 hover:bg-content/5 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-60 ${open ? "border-content/20 bg-content/5" : "border-content/10"}`}
    >
      {status?.icon ?? (
        <Chatting
          className="size-4 shrink-0 text-content/45"
          strokeWidth={1.75}
        />
      )}
      <span className="min-w-0 truncate text-[13px] text-content">{title}</span>
      {/* Squeezed, the title gives way first and the project after it. */}
      <span
        className="flex min-w-0 max-w-[45%] shrink-0 items-center gap-1 rounded-full border border-content/10 bg-content/5 px-2 py-0.5 text-[12px] text-content/65"
        title={launch.cwd}
      >
        <HarnessIcon harness={launch.harness} className="size-3.5 shrink-0" />
        <span className="min-w-0 truncate">{projectName(launch.cwd)}</span>
      </span>
    </button>
  );
}
