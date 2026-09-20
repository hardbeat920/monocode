import type { HarnessId } from "./session";
import { HARNESSES } from "./session";
import type { SessionSummary } from "../data/sessionStore";

export type SessionTimeFilter = "all" | "today" | "7d" | "30d";

export type SessionStatusFilter = {
  working: boolean;
  needsApproval: boolean;
  done: boolean;
};

export type SessionSidebarFilters = {
  showArchived: boolean;
  hiddenHarnesses: HarnessId[];
  time: SessionTimeFilter;
  status: SessionStatusFilter;
};

export const DEFAULT_SESSION_STATUS_FILTER: SessionStatusFilter = {
  working: false,
  needsApproval: false,
  done: false,
};

export const DEFAULT_SESSION_SIDEBAR_FILTERS: SessionSidebarFilters = {
  showArchived: false,
  hiddenHarnesses: [],
  time: "all",
  status: DEFAULT_SESSION_STATUS_FILTER,
};

const FILTERS_KEY = "monocode.sessionSidebarFilters";

export function harnessesInSessions(rows: SessionSummary[]): HarnessId[] {
  const seen = new Set<HarnessId>();
  for (const row of rows) seen.add(row.harness);
  return HARNESSES.filter((harness) => seen.has(harness));
}

export function loadSessionSidebarFilters(): SessionSidebarFilters {
  try {
    const raw = localStorage.getItem(FILTERS_KEY);
    if (!raw) {
      const legacyArchived =
        localStorage.getItem("monocode.sessionsShowArchived") === "1";
      return legacyArchived
        ? { ...DEFAULT_SESSION_SIDEBAR_FILTERS, showArchived: true }
        : DEFAULT_SESSION_SIDEBAR_FILTERS;
    }
    const parsed = JSON.parse(raw) as Partial<SessionSidebarFilters>;
    return {
      showArchived: parsed.showArchived === true,
      hiddenHarnesses: Array.isArray(parsed.hiddenHarnesses)
        ? parsed.hiddenHarnesses.filter(isHarnessId)
        : [],
      time: isTimeFilter(parsed.time) ? parsed.time : "all",
      status: {
        working: parsed.status?.working === true,
        needsApproval: parsed.status?.needsApproval === true,
        done: parsed.status?.done === true,
      },
    };
  } catch {
    return DEFAULT_SESSION_SIDEBAR_FILTERS;
  }
}

export function saveSessionSidebarFilters(filters: SessionSidebarFilters) {
  try {
    localStorage.setItem(FILTERS_KEY, JSON.stringify(filters));
  } catch {
    // private mode / quota
  }
}

export function hasActiveSessionFilters(
  filters: SessionSidebarFilters,
): boolean {
  return (
    filters.showArchived ||
    filters.hiddenHarnesses.length > 0 ||
    filters.time !== "all" ||
    filters.status.working ||
    filters.status.needsApproval ||
    filters.status.done
  );
}

export function filterSessionsByHarness(
  rows: SessionSummary[],
  hiddenHarnesses: Iterable<HarnessId>,
): SessionSummary[] {
  const hidden = new Set(hiddenHarnesses);
  if (hidden.size === 0) return rows;
  return rows.filter((row) => !hidden.has(row.harness));
}

export function filterSessionsByTime(
  rows: SessionSummary[],
  time: SessionTimeFilter,
  now: number,
): SessionSummary[] {
  if (time === "all") return rows;
  const start = timeFilterStart(time, now);
  return rows.filter((row) => row.updatedAt >= start);
}

export function filterSessionsByStatus(
  rows: SessionSummary[],
  status: SessionStatusFilter,
  busyIds: Set<string>,
  approvalIds: Set<string>,
  doneIds: Set<string>,
): SessionSummary[] {
  const any = status.working || status.needsApproval || status.done;
  if (!any) return rows;
  return rows.filter((row) => {
    if (status.working && busyIds.has(row.id)) return true;
    if (status.needsApproval && approvalIds.has(row.id)) return true;
    if (status.done && doneIds.has(row.id)) return true;
    return false;
  });
}

export function timeFilterStart(time: SessionTimeFilter, now: number): number {
  if (time === "today") {
    const date = new Date(now);
    date.setHours(0, 0, 0, 0);
    return date.getTime();
  }
  const dayMs = 24 * 60 * 60 * 1000;
  if (time === "7d") return now - 7 * dayMs;
  if (time === "30d") return now - 30 * dayMs;
  return 0;
}

function isHarnessId(value: unknown): value is HarnessId {
  return typeof value === "string" && (HARNESSES as string[]).includes(value);
}

function isTimeFilter(value: unknown): value is SessionTimeFilter {
  return value === "all" || value === "today" || value === "7d" || value === "30d";
}
