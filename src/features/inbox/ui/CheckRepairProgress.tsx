import { useId, useState, useSyncExternalStore } from "react";
import {
  getCiRepairs,
  subscribeCiRepairs,
  type TrackedCiRepair,
} from "../model/ciRepairTracking";
import { sameProjectPath } from "../../projects/model/recents";
import {
  CheckCircle,
  ChevronRight,
  CircleDashed,
  CircleX,
  LoaderCircle,
  MessageSquare,
  type IconComponent,
} from "../../../shared/ui/icons";
import type { CheckRepair } from "./CheckRepairForm";
import type { GithubPrChecksView } from "../hooks/useGithubPrChecks";
import type {
  GithubPrCheck,
  GithubPrChecks,
  GithubPrCheckState,
} from "../model/githubPrChecks";
import { githubActionsJobId } from "../model/githubPrChecks";
import { useT } from "../../../shared/i18n/useT";
import type { TextKey } from "../../../shared/i18n/catalog";

type RepairState =
  | GithubPrCheckState
  | "repairing"
  | "waiting"
  | "stale"
  | "refreshing"
  | "stopped"
  | "interrupted"
  | "agent-error";
type RepairItem = {
  attempt: TrackedCiRepair;
  check: TrackedCiRepair["checks"][number];
  state: RepairState;
};
export type RepairGroup = { sessionId: string; items: RepairItem[] };

export function findCheckRepair(
  groups: RepairGroup[],
  check: GithubPrCheck,
  current: GithubPrChecks,
): RepairItem | undefined {
  const candidates = groups
    .flatMap((group) => group.items)
    .filter(
      (item) =>
        item.check.name === check.name &&
        item.check.workflow === check.workflow,
    );
  const exact = candidates.find(
    (item) =>
      item.attempt.headOid === current.headOid && item.check.url === check.url,
  );
  if (exact) return exact;
  // A new commit changes job URLs. Match by name only when both sides are unique.
  if (
    candidates.length !== 1 ||
    candidates[0].attempt.headOid === current.headOid
  )
    return undefined;
  return current.checks.filter(
    (item) => item.name === check.name && item.workflow === check.workflow,
  ).length === 1
    ? candidates[0]
    : undefined;
}

function repairState(
  attempt: TrackedCiRepair,
  check: RepairItem["check"],
  view: GithubPrChecksView,
  ambiguous: boolean,
): RepairState {
  if (attempt.phase === "running") return "repairing";
  if (attempt.phase === "failed") return "agent-error";
  if (attempt.phase === "cancelled") return "stopped";
  if (attempt.phase === "interrupted") return "interrupted";
  if (view.stale || view.error) return "stale";
  if (view.loading || view.refreshing) return "refreshing";
  const current = view.checks;
  if (!current || current.headOid === attempt.headOid || ambiguous)
    return "waiting";
  const matches = current.checks.filter(
    (item) => item.name === check.name && item.workflow === check.workflow,
  );
  const originals = attempt.checks.filter(
    (item) => item.name === check.name && item.workflow === check.workflow,
  );
  if (matches.length !== 1 || originals.length !== 1) return "waiting";
  const latest = matches[0];
  // Only a distinct, newer job can verify a completed repair attempt.
  if (
    (githubActionsJobId(check.url, attempt.repo) && latest.url === check.url) ||
    !latest.startedAt ||
    !Number.isFinite(Date.parse(latest.startedAt)) ||
    Date.parse(latest.startedAt) < attempt.startedAt
  )
    return "waiting";
  return latest.state;
}

export function useCheckRepairs(
  cwd: string,
  repo: string,
  number: number | undefined,
  view: GithubPrChecksView,
): RepairGroup[] {
  const attempts = useSyncExternalStore(
    subscribeCiRepairs,
    getCiRepairs,
    getCiRepairs,
  );
  const seen = new Map<string, { headOid: string; urls: Set<string | null> }>();
  const groups = new Map<string, RepairGroup>();
  const counts = new Map<string, number>();
  for (const attempt of attempts) {
    if (
      !sameProjectPath(attempt.cwd, cwd) ||
      attempt.repo.toLowerCase() !== repo.toLowerCase() ||
      attempt.number !== number
    )
      continue;
    for (const check of attempt.checks) {
      const key = JSON.stringify([check.workflow, check.name]);
      const previous = seen.get(key);
      if (
        previous &&
        (previous.headOid !== attempt.headOid || previous.urls.has(check.url))
      )
        continue;
      if (previous) previous.urls.add(check.url);
      else
        seen.set(key, { headOid: attempt.headOid, urls: new Set([check.url]) });
      const group = groups.get(attempt.sessionId) ?? {
        sessionId: attempt.sessionId,
        items: [],
      };
      group.items.push({
        attempt,
        check,
        state: "waiting",
      });
      counts.set(key, (counts.get(key) ?? 0) + 1);
      groups.set(attempt.sessionId, group);
    }
  }
  for (const group of groups.values()) {
    for (const item of group.items) {
      const key = JSON.stringify([item.check.workflow, item.check.name]);
      item.state = repairState(
        item.attempt,
        item.check,
        view,
        (counts.get(key) ?? 0) > 1,
      );
    }
  }
  return [...groups.values()];
}

const neutral = "text-content/55";
const positive = "text-emerald-700 dark:text-emerald-400";
const negative = "text-rose-700 dark:text-rose-400";
const active = "text-amber-700 dark:text-amber-400";
const states: Record<
  RepairState,
  {
    label: TextKey;
    summary: TextKey;
    count: TextKey;
    Icon: IconComponent;
    color: string;
  }
> = {
  repairing: {
    label: "inbox.repair.state.repairing.label",
    summary: "inbox.repair.state.repairing.summary",
    count: "inbox.repair.state.repairing.count",
    Icon: LoaderCircle,
    color: active,
  },
  waiting: {
    label: "inbox.repair.state.waiting.label",
    summary: "inbox.repair.state.waiting.summary",
    count: "inbox.repair.state.waiting.count",
    Icon: CircleDashed,
    color: neutral,
  },
  refreshing: {
    label: "inbox.repair.state.refreshing.label",
    summary: "inbox.repair.state.refreshing.summary",
    count: "inbox.repair.state.refreshing.count",
    Icon: LoaderCircle,
    color: neutral,
  },
  stale: {
    label: "inbox.repair.state.stale.label",
    summary: "inbox.repair.state.stale.summary",
    count: "inbox.repair.state.stale.count",
    Icon: CircleDashed,
    color: neutral,
  },
  pass: {
    label: "inbox.repair.state.pass.label",
    summary: "inbox.repair.state.pass.summary",
    count: "inbox.repair.state.pass.count",
    Icon: CheckCircle,
    color: positive,
  },
  fail: {
    label: "inbox.repair.state.fail.label",
    summary: "inbox.repair.state.fail.summary",
    count: "inbox.repair.state.fail.count",
    Icon: CircleX,
    color: negative,
  },
  pending: {
    label: "inbox.repair.state.pending.label",
    summary: "inbox.repair.state.pending.summary",
    count: "inbox.repair.state.pending.count",
    Icon: LoaderCircle,
    color: active,
  },
  cancel: {
    label: "inbox.repair.state.cancel.label",
    summary: "inbox.repair.state.cancel.summary",
    count: "inbox.repair.state.cancel.count",
    Icon: CircleDashed,
    color: neutral,
  },
  skipping: {
    label: "inbox.repair.state.skipping.label",
    summary: "inbox.repair.state.skipping.summary",
    count: "inbox.repair.state.skipping.count",
    Icon: CircleDashed,
    color: neutral,
  },
  unknown: {
    label: "inbox.repair.state.unknown.label",
    summary: "inbox.repair.state.unknown.summary",
    count: "inbox.repair.state.unknown.count",
    Icon: CircleDashed,
    color: neutral,
  },
  stopped: {
    label: "inbox.repair.state.stopped.label",
    summary: "inbox.repair.state.stopped.summary",
    count: "inbox.repair.state.stopped.count",
    Icon: CircleDashed,
    color: neutral,
  },
  interrupted: {
    label: "inbox.repair.state.interrupted.label",
    summary: "inbox.repair.state.interrupted.summary",
    count: "inbox.repair.state.interrupted.count",
    Icon: CircleDashed,
    color: neutral,
  },
  "agent-error": {
    label: "inbox.repair.state.agentError.label",
    summary: "inbox.repair.state.agentError.summary",
    count: "inbox.repair.state.agentError.count",
    Icon: CircleX,
    color: negative,
  },
};
function StatusIcon({ state }: { state: RepairState }) {
  const { Icon, color } = states[state];
  return (
    <Icon
      aria-hidden="true"
      strokeWidth={1.75}
      className={`size-3.5 shrink-0 ${color} ${["repairing", "refreshing", "pending"].includes(state) ? "animate-spin motion-reduce:animate-none" : ""}`}
    />
  );
}

export function CheckRepairStatus({ item }: { item: RepairItem }) {
  const t = useT();
  const status = states[item.state];
  return (
    <span
      data-repair-status
      role="status"
      title={t(status.summary)}
      className={`inline-flex shrink-0 items-center gap-1.5 text-[11px] font-medium ${status.color}`}
    >
      <StatusIcon state={item.state} />
      {t(status.label)}
    </span>
  );
}

function RepairCard({
  group,
  repair,
  view,
  onShowCheck,
}: {
  group: RepairGroup;
  repair: CheckRepair;
  view: GithubPrChecksView;
  onShowCheck?: (check: RepairItem["check"]) => void;
}) {
  const t = useT();
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const counts = new Map<RepairState, number>();
  for (const item of group.items)
    counts.set(item.state, (counts.get(item.state) ?? 0) + 1);
  const priority: RepairState[] = [
    "repairing",
    "agent-error",
    "fail",
    "stale",
    "refreshing",
    "pending",
    "waiting",
    "interrupted",
    "stopped",
    "cancel",
    "unknown",
    "skipping",
    "pass",
  ];
  const lead = priority.find((state) => counts.has(state))!;
  const label = t("inbox.repair.checkCount", { count: group.items.length });
  const single = group.items.length === 1 ? group.items[0] : undefined;
  const canShowCheck =
    single?.state === "pass" &&
    onShowCheck &&
    view.checks?.checks.filter(
      (check) =>
        check.name === single.check.name &&
        check.workflow === single.check.workflow,
    ).length === 1;
  return (
    <div className="overflow-hidden rounded-lg border border-stroke bg-content/[0.02]">
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5">
        <button
          type="button"
          aria-label={t("inbox.repair.detailsFor", { label })}
          aria-expanded={expanded}
          aria-controls={detailsId}
          onClick={() => setExpanded(!expanded)}
          className="flex min-w-0 flex-1 items-center gap-2.5 rounded text-left focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
        >
          <StatusIcon state={lead} />
          <span className="min-w-0 flex-1">
            <span
              role="status"
              className="flex flex-wrap gap-x-3 gap-y-0.5 text-[12px] font-medium"
            >
              {[...counts].map(([state, count]) => (
                <span key={state} className={states[state].color}>
                  {[
                    "pass",
                    "fail",
                    "pending",
                    "cancel",
                    "skipping",
                    "unknown",
                  ].includes(state)
                    ? single
                      ? t(states[state].label)
                      : t(states[state].count, { count })
                    : counts.size === 1
                      ? t(states[state].summary)
                      : t(states[state].count, { count })}
                </span>
              ))}
            </span>
            <span
              title={
                single
                  ? `${single.check.workflow}: ${single.check.name}`
                  : undefined
              }
              className="mt-0.5 block truncate text-[11px] text-content/65"
            >
              {single
                ? single.check.name
                : t("inbox.repair.ciRepairFor", { label })}
            </span>
          </span>
          <ChevronRight
            aria-hidden="true"
            className={`size-3 shrink-0 text-content/40 transition-transform motion-reduce:transition-none ${expanded ? "rotate-90" : ""}`}
          />
        </button>
        {canShowCheck ? (
          <button
            type="button"
            onClick={() => onShowCheck?.(single.check)}
            className="shrink-0 rounded-md px-2 py-1.5 text-[11px] text-content/70 hover:bg-selection hover:text-content focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
          >
            {t("inbox.repair.showCheck")}
          </button>
        ) : null}
        {repair.onOpenSession ? (
          <button
            type="button"
            onClick={() => void repair.onOpenSession?.(group.sessionId)}
            className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-stroke px-2.5 py-1.5 text-[11px] text-content/70 hover:bg-selection hover:text-content focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
          >
            <MessageSquare aria-hidden="true" className="size-3.5" />
            {t("inbox.repair.openConversation")}
          </button>
        ) : null}
      </div>
      {expanded ? (
        <div
          id={detailsId}
          className="space-y-2 border-t border-stroke px-3 py-2.5 text-[11px] text-content/55"
        >
          <p>{t("inbox.repair.includedChecks")}</p>
          <ul className="flex max-h-36 flex-wrap gap-1.5 overflow-y-auto">
            {group.items.map(({ check }) => (
              <li
                key={JSON.stringify([check.workflow, check.name, check.url])}
                title={check.workflow}
                className="max-w-full truncate rounded bg-content/5 px-2 py-1 text-content/75"
              >
                {check.name}
              </li>
            ))}
          </ul>
          <p>
            {t("inbox.repair.latestCommit")}{" "}
            <span className="font-mono">
              {view.checks?.headOid.slice(0, 7) ||
                t("inbox.repair.unavailable")}
            </span>
            . {t("inbox.repair.resultsBelow")}
          </p>
        </div>
      ) : null}
    </div>
  );
}

export function CheckRepairProgress({
  cwd,
  repo,
  repair,
  view,
  onShowCheck,
}: {
  cwd: string;
  repo: string;
  repair: CheckRepair;
  view: GithubPrChecksView;
  onShowCheck?: (check: RepairItem["check"]) => void;
}) {
  const t = useT();
  const groups = useCheckRepairs(cwd, repo, repair.number, view);
  if (!groups.length) return null;
  return (
    <div className="space-y-2" aria-label={t("inbox.repair.progress")}>
      {groups.map((group) => (
        <RepairCard
          key={group.sessionId}
          group={group}
          repair={repair}
          view={view}
          onShowCheck={onShowCheck}
        />
      ))}
    </div>
  );
}
