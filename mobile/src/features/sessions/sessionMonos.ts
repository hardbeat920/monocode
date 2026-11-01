import type { Mono, SessionSummary } from "@/features/workspace/types";

/** The Monos a project's Sessions page pins above its rows. */
export type MonoGroup = {
  /** Assigned to this project, the one heard from last first. */
  assigned: Mono[];
  /** Assigned to no project; listed on the first project's page only. */
  unassigned: Mono[];
};

const byActivity = (a: Mono, b: Mono) => (b.lastActiveAt || 0) - (a.lastActiveAt || 0);

/**
 * Which Monos a project's page pins. One assigned to several projects is in
 * each of them. One assigned to none has no page of its own, so it is pinned,
 * as "Unassigned", on the first project's page: the one the app falls back to
 * when it has nowhere else to open.
 */
export function monosForProject(
  monos: readonly Mono[],
  projects: readonly { id: string }[],
  projectId: string,
): MonoGroup {
  const first = projects[0]?.id === projectId;
  return {
    assigned: monos.filter((mono) => mono.projectIds.includes(projectId)).sort(byActivity),
    unassigned: first ? monos.filter((mono) => mono.projectIds.length === 0).sort(byActivity) : [],
  };
}

/** The group narrowed by the page's search, which matches a Mono's name or last line. */
export function searchMonos(group: MonoGroup, needle: string): MonoGroup {
  if (!needle) return group;
  const match = (mono: Mono) =>
    mono.name.toLowerCase().includes(needle) || mono.lastMessage.toLowerCase().includes(needle);
  return { assigned: group.assigned.filter(match), unassigned: group.unassigned.filter(match) };
}

export const groupSize = (group: MonoGroup) => group.assigned.length + group.unassigned.length;

/** Where a session came from, for the chip on its row. */
export type SessionOrigin =
  /** Started by a Mono the phone knows; `openable` when its chat is on this phone. */
  | { kind: "mono"; mono: Mono; openable: boolean }
  /** Started by a Mono the desktop no longer has. */
  | { kind: "removed" };

/**
 * Who started a session, when the catalog says so. A session with no `monoId`
 * has no origin: it was started by hand, or adopted into a Mono's care, and
 * the desktop tells those apart from nothing. Naming a Mono the catalog
 * doesn't list means it was deleted.
 *
 * `all` is every Mono on the desktop; `shown` those whose chats this phone
 * lists, a subset when some Mono's projects aren't picked.
 */
export function sessionOrigin(
  session: Pick<SessionSummary, "monoId">,
  all: ReadonlyMap<string, Mono>,
  shown: ReadonlySet<string>,
): SessionOrigin | null {
  if (!session.monoId) return null;
  const mono = all.get(session.monoId);
  return mono ? { kind: "mono", mono, openable: shown.has(mono.id) } : { kind: "removed" };
}

/**
 * The sessions "Archive all finished" would take from a Mono: its own, in
 * the list shown, that are neither working nor waiting on the user.
 */
export function finishedFromMono(
  sessions: readonly SessionSummary[],
  monoId: string,
): SessionSummary[] {
  return sessions.filter(
    (session) => session.monoId === monoId && !session.busy && !session.waiting,
  );
}

/** Whether two origins draw the same chip, so a fresh catalog's new objects don't redraw rows. */
export function sameOrigin(a: SessionOrigin | null, b: SessionOrigin | null) {
  if (a === b) return true;
  if (!a || !b || a.kind !== b.kind) return false;
  if (a.kind === "removed" || b.kind === "removed") return true;
  return (
    a.openable === b.openable &&
    a.mono.id === b.mono.id &&
    a.mono.name === b.mono.name &&
    a.mono.mascot === b.mono.mascot &&
    a.mono.color === b.mono.color
  );
}
