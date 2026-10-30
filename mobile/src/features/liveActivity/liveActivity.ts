import { requireOptionalNativeModule } from "expo";
import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

import type { Pairing } from "@/features/connection/nativeLink";
import { projectMascot } from "@/features/mascots/projectMascots";
import type { Project, SessionSummary } from "@/features/workspace/types";

/**
 * The Live Activity for the paired desktop's sessions, drawn by the widget in
 * targets/live-activity. The app redraws it from the catalog while open, and
 * from the session list the desktop's pushes carry while it is not.
 */

export type ActivityStatus = "working" | "approval" | "question" | "done";

/** A session as the desktop's pushes carry it (notify::activity). */
export type ActivitySession = {
  id: string;
  projectId: string;
  project: string;
  /** Draw the project's mascot, as the project rail does. */
  projectPath?: string;
  color?: string;
  mascot?: string;
  title: string;
  model: string;
  status: ActivityStatus;
  /** What it waits on, while it waits. */
  request?: { id: string; label: string; count: number };
};

type NativeActivity = {
  isEnabled(): boolean;
  rememberLink(link: string): void;
  forgetLink(): void;
  sync(
    desktop: string,
    sessions: (Omit<ActivitySession, "projectPath" | "mascot" | "request"> & {
      since: number;
      color: string;
      grid: string;
      request: string;
      requestId: string;
      requests: number;
    })[],
  ): Promise<boolean>;
  end(): Promise<void>;
};
const native = requireOptionalNativeModule<NativeActivity>("MonocodeActivity");

/** Whether this phone can show one: iOS, a build with the widget, and
 *  Live Activities allowed for the app in system settings. */
export function liveActivitiesAvailable() {
  return native?.isEnabled() ?? false;
}

const URGENCY: Record<ActivityStatus, number> = {
  approval: 0,
  question: 1,
  working: 2,
  done: 3,
};
const MAX_SESSIONS = 4;

/**
 * The activity's sessions after `before` became `after`: each working or
 * waiting, and those that just finished. As the desktop's notify::activity,
 * which builds the same list for its pushes.
 */
export function activitySessions(
  before: readonly SessionSummary[] | null,
  after: readonly SessionSummary[],
  projects: readonly Pick<Project, "id" | "name" | "path" | "color" | "mascot">[],
): ActivitySession[] {
  const last = new Map(before?.map((session) => [session.id, session]));
  const sessions: ActivitySession[] = [];
  for (const session of after) {
    const previous = last.get(session.id);
    const status: ActivityStatus | null =
      session.waiting === "approval"
        ? "approval"
        : session.waiting
          ? "question"
          : session.busy
            ? "working"
            : previous?.busy && !previous.waiting
              ? "done"
              : null;
    if (!status) continue;
    const project = projects.find((p) => p.id === session.projectId);
    sessions.push({
      id: session.id,
      projectId: session.projectId,
      project: project?.name ?? "",
      projectPath: project?.path ?? "",
      color: project?.color ?? "",
      mascot: project?.mascot ?? "",
      title: session.title.slice(0, 60),
      model: session.modelName ?? "",
      status,
      ...(session.waiting && session.request ? { request: session.request } : {}),
    });
  }
  return sessions.slice(0, MAX_SESSIONS);
}

/** The pushed sessions, wherever Expo put the push's data. */
export function pushedActivity(payload: unknown): ActivitySession[] | null {
  const seen = new Set<unknown>();
  const search = (value: unknown, depth: number): ActivitySession[] | null => {
    if (depth > 4 || !value || seen.has(value)) return null;
    if (typeof value === "string") {
      if (!value.startsWith("{")) return null;
      try {
        return search(JSON.parse(value), depth + 1);
      } catch {
        return null;
      }
    }
    if (typeof value !== "object") return null;
    seen.add(value);
    const activity = (value as { activity?: unknown }).activity;
    if (Array.isArray(activity)) return activity as ActivitySession[];
    for (const child of Object.values(value)) {
      const found = search(child, depth + 1);
      if (found) return found;
    }
    return null;
  };
  return search(payload, 0);
}

// When each session started working, kept across launches so a woken app
// counts from the same moment the open one did.
const SINCE_KEY = "liveActivity.since";

function startTimes(sessions: readonly ActivitySession[], now: number) {
  let saved: Record<string, number> = {};
  try {
    saved = JSON.parse(Storage.getItemSync(SINCE_KEY) ?? "{}");
  } catch {}
  const next: Record<string, number> = {};
  for (const session of sessions) next[session.id] = saved[session.id] ?? now;
  Storage.setItemSync(SINCE_KEY, JSON.stringify(next));
  return next;
}

// Serialized, as a push can land while the app is redrawing.
let queue = Promise.resolve();

/** Redraws the activity: starts it if there is none (only in the foreground),
 *  or ends it once nothing works or waits. */
export function showSessions(desktop: string, sessions: ActivitySession[]) {
  if (!native) return queue;
  const sorted = [...sessions].sort(
    (a, b) => URGENCY[a.status] - URGENCY[b.status],
  );
  queue = queue
    .then(async () => {
      const since = startTimes(sorted, Date.now() / 1000);
      await native.sync(
        desktop,
        sorted.map(({ projectPath, mascot, request, ...session }) => ({
          ...session,
          request: request?.label ?? "",
          requestId: request?.id ?? "",
          requests: request?.count ?? 0,
          since: since[session.id],
          color: session.color || "#ffffff",
          grid: projectMascot(projectPath || session.projectId, mascot).rest.join(""),
        })),
      );
    })
    .catch((reason) => console.warn("Could not update the Live Activity", reason));
  return queue;
}

export function endActivity() {
  queue = queue.then(() => native?.end()).catch(() => {});
  return queue;
}

/** Lets the activity's Approve button reach the desktop from native code,
 *  with or without this JavaScript running. */
export function rememberLink(pairing: Pairing | null) {
  if (!native) return;
  if (!pairing) return native.forgetLink();
  native.rememberLink(
    JSON.stringify({
      identity: pairing.identity,
      pin: pairing.desktop.fingerprint,
      addresses: pairing.desktop.addresses,
    }),
  );
}

/** The desktop's name, for a woken app that has not loaded its pairing. */
const DESKTOP_KEY = "liveActivity.desktop";
export const rememberDesktop = (name: string) =>
  Storage.setItemSync(DESKTOP_KEY, name);
export const rememberedDesktop = () =>
  Storage.getItemSync(DESKTOP_KEY) ?? "Desktop";

// The setting: on unless turned off.
const OFF_KEY = "liveActivity.off";
let off: boolean | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

export function liveActivityEnabled() {
  return !(off ??= Storage.getItemSync(OFF_KEY) === "1");
}

export function useLiveActivityPreference() {
  const enabled = useSyncExternalStore(subscribe, liveActivityEnabled);
  const setEnabled = (on: boolean) => {
    off = !on;
    Storage.setItemSync(OFF_KEY, on ? "0" : "1");
    listeners.forEach((listener) => listener());
  };
  return [enabled, setEnabled] as const;
}
