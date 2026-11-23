import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { projectName } from "../../../shared/lib/paths";
import {
  HARNESS_TITLE,
  sessionDisplayTitle,
  type Session,
} from "../../sessions/model/session";
import { readFlag, writeFlag } from "./storageFlags";

const KEY = "monocode.discordPresence";

/** Devtools tracing in the real dev server, but not in test output. */
const TRACE = import.meta.env.DEV && import.meta.env.MODE !== "test";

/** Off until the user opts in: presence broadcasts what they are working on. */
export const DISCORD_PRESENCE_DEFAULT = false;

export const DISCORD_PRESENCE_CHANGE_EVENT = "monocode:discord-presence-change";

export function loadDiscordPresenceEnabled(): boolean {
  return readFlag(KEY) ?? DISCORD_PRESENCE_DEFAULT;
}

export function saveDiscordPresenceEnabled(value: boolean) {
  writeFlag(KEY, value);
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(DISCORD_PRESENCE_CHANGE_EVENT, { detail: value }),
  );
}

export function subscribeDiscordPresence(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (event.key === KEY) onStoreChange();
  };
  window.addEventListener(DISCORD_PRESENCE_CHANGE_EVENT, onStoreChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(DISCORD_PRESENCE_CHANGE_EVENT, onStoreChange);
    window.removeEventListener("storage", onStorage);
  };
}

export function useDiscordPresenceEnabled(): boolean {
  return useSyncExternalStore(
    subscribeDiscordPresence,
    loadDiscordPresenceEnabled,
    () => DISCORD_PRESENCE_DEFAULT,
  );
}

/** Mirrors `discord::PresenceStatus` in src-tauri/src/discord.rs. */
export type DiscordPresenceStatus =
  | "disabled"
  | "connecting"
  | "connected"
  | "unavailable"
  | "error";

export const DISCORD_STATUS_LABEL: Record<DiscordPresenceStatus, string> = {
  disabled: "Off",
  connecting: "Connecting…",
  connected: "Connected",
  unavailable: "Discord not running",
  error: "Connection lost",
};

const STATUSES = new Set<string>(Object.keys(DISCORD_STATUS_LABEL));

function parseStatus(value: unknown): DiscordPresenceStatus {
  return typeof value === "string" && STATUSES.has(value)
    ? (value as DiscordPresenceStatus)
    : "error";
}

export type DiscordActivity = {
  state: string;
  details: string;
  /** Unix epoch milliseconds; Discord expects the start in ms. */
  startedAt: number;
};

/** Discord allows at most 128 bytes for activity details. */
const DETAILS_MAX_BYTES = 128;
const IDLE_SUFFIX = " · idle";
const encoder = new TextEncoder();

function utf8Length(value: string): number {
  return encoder.encode(value).length;
}

/** Trim at a character boundary so the result never exceeds `maxBytes`. */
export function truncateUtf8(value: string, maxBytes: number): string {
  let result = "";
  let bytes = 0;
  for (const character of value) {
    const size = utf8Length(character);
    if (bytes + size > maxBytes) break;
    result += character;
    bytes += size;
  }
  return result;
}

let lastActivity: DiscordActivity | null = null;

/** Keep the last computed activity so the Settings toggle can resend it. */
export function rememberDiscordActivity(activity: DiscordActivity | null) {
  lastActivity = activity;
}

/** Pushes the current activity (or the remembered one) to the presence client. */
export async function setDiscordPresence(
  enabled: boolean,
  activity: DiscordActivity | null = lastActivity,
): Promise<DiscordPresenceStatus> {
  if (TRACE) {
    console.debug("[monocode:discord] set", { enabled, activity });
  }
  try {
    const status = parseStatus(
      await invoke<DiscordPresenceStatus>("discord_presence_set", {
        enabled,
        activity,
      }),
    );
    if (TRACE) {
      console.debug("[monocode:discord] status", status);
    }
    return status;
  } catch (error) {
    if (TRACE) {
      console.debug("[monocode:discord] command failed", error);
    }
    return "error";
  }
}

export async function discordPresenceStatus(): Promise<DiscordPresenceStatus> {
  try {
    return parseStatus(
      await invoke<DiscordPresenceStatus>("discord_presence_status"),
    );
  } catch (error) {
    if (TRACE) {
      console.debug("[monocode:discord] status probe failed", error);
    }
    return "error";
  }
}

export function buildActivity(
  active: Session | undefined,
  cwd: string,
  busyCount: number,
  startedAt: number,
): DiscordActivity {
  const project = projectName(active?.cwd ?? cwd);
  const place = project || "MonoCode";
  if (!active) {
    const details =
      busyCount > 0
        ? `${busyCount} agent${busyCount === 1 ? "" : "s"} running`
        : "Browsing MonoCode";
    return {
      state: `In ${place}`,
      details: truncateUtf8(details, DETAILS_MAX_BYTES),
      startedAt,
    };
  }
  const title = sessionDisplayTitle(active.title, active.harness);
  // Reserve room for the idle suffix before truncating, so the final value
  // (suffix included) still fits in the 128-byte limit.
  const details = active.busy
    ? truncateUtf8(title, DETAILS_MAX_BYTES)
    : `${truncateUtf8(
        title,
        DETAILS_MAX_BYTES - utf8Length(IDLE_SUFFIX),
      )}${IDLE_SUFFIX}`;
  return {
    state: `${HARNESS_TITLE[active.harness]} · ${place}`,
    details,
    startedAt,
  };
}

/**
 * Publishes the focused session as Discord Rich Presence. Called once from the
 * app shell: the enabled flag is an external store, so toggling it in General
 * settings is enough to connect or disconnect without threading props here.
 */
export function useDiscordPresence(
  active: Session | undefined,
  sessions: readonly Session[],
  cwd: string,
  focused = true,
  focusToken = 0,
): DiscordPresenceStatus {
  const enabled = useDiscordPresenceEnabled();
  const [status, setStatus] = useState<DiscordPresenceStatus>(() =>
    enabled ? "connecting" : "disabled",
  );
  const timer = useRef<{ key: string; at: number }>({ key: "", at: 0 });
  const busyCount = sessions.reduce(
    (count, session) => count + (session.busy ? 1 : 0),
    0,
  );

  const key = JSON.stringify([
    enabled,
    active?.id ?? null,
    active?.title ?? null,
    active?.harness ?? null,
    !!active?.busy,
    active?.cwd ?? cwd,
    busyCount,
  ]);

  useEffect(() => {
    // Several MonoCode windows share one presence client. Only the focused one
    // publishes, and a focus change republishes even if nothing else changed.
    if (!focused) return;
    if (timer.current.key !== key) {
      timer.current = { key, at: Date.now() };
    }
    const activity = buildActivity(active, cwd, busyCount, timer.current.at);
    rememberDiscordActivity(activity);
    void setDiscordPresence(enabled, activity).then(setStatus);
    // `key` already captures every field of `active` and `sessions` used above.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, focused, focusToken, key]);

  return status;
}
