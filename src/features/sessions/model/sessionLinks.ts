import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useSyncExternalStore } from "react";

/** One stored link between two sessions, as `session_links_list` returns it. */
export type SessionLink = {
  a: string;
  b: string;
  aTitle: string;
  bTitle: string;
  agentMessages: number;
  createdAt: number;
};

export type LinkedPeer = {
  id: string;
  title: string;
};

/** Agent messages allowed per link before a user has to write again. */
export const LINK_MESSAGE_LIMIT = 5;

const CHANGED_EVENT = "monocode:session-links-changed";

export function peersOf(links: readonly SessionLink[], id: string): LinkedPeer[] {
  const peers: LinkedPeer[] = [];
  for (const link of links) {
    if (link.a === id) peers.push({ id: link.b, title: link.bTitle });
    else if (link.b === id) peers.push({ id: link.a, title: link.aTitle });
  }
  return peers;
}

export function areLinked(
  links: readonly SessionLink[],
  first: string,
  second: string,
): boolean {
  return links.some(
    (link) =>
      (link.a === first && link.b === second) ||
      (link.a === second && link.b === first),
  );
}

/** What the receiving agent reads when a linked session messages it. */
export function linkedMessagePrompt(from: LinkedPeer, message: string): string {
  const title = from.title.trim() || "Untitled session";
  return `<linked_session_message from_session_id="${from.id}" from_title="${title.replace(/"/g, "'")}">
This message comes from the agent in a linked MonoCode session, not from the user. Treat it as a request from a collaborator. To answer, use the MonoCode app CLI action links.send with that session ID.

${message.trim()}
</linked_session_message>`;
}

let links: SessionLink[] = [];
let loaded: Promise<SessionLink[]> | null = null;
const listeners = new Set<() => void>();
let watching = false;

function publish(next: SessionLink[]) {
  links = next;
  for (const listener of listeners) listener();
}

function available(): boolean {
  return typeof isTauri === "function" && isTauri();
}

export function refreshSessionLinks(): Promise<SessionLink[]> {
  if (!available()) return Promise.resolve(links);
  loaded = invoke<SessionLink[]>("session_links_list")
    .then((next) => {
      publish(next);
      return next;
    })
    .catch(() => links);
  return loaded;
}

/** The current links, loading them on first use. */
export function loadSessionLinks(): Promise<SessionLink[]> {
  return loaded ?? refreshSessionLinks();
}

export function peekSessionLinks(): SessionLink[] {
  return links;
}

function watch() {
  if (watching || !available()) return;
  watching = true;
  void listen(CHANGED_EVENT, () => void refreshSessionLinks()).catch(() => {
    watching = false;
  });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getLinks() {
  return links;
}

export function useSessionLinks(): SessionLink[] {
  useEffect(() => {
    watch();
    void loadSessionLinks();
  }, []);
  return useSyncExternalStore(subscribe, getLinks, getLinks);
}

export async function linkSessions(first: string, second: string) {
  await invoke("session_link", { first, second });
  await refreshSessionLinks();
}

export async function unlinkSessions(first: string, second: string) {
  await invoke("session_unlink", { first, second });
  await refreshSessionLinks();
}

/** One counted agent message. Pass it to `releaseLinkedMessage` to give it back. */
export type LinkReservation = { count: number; epoch: number };

/** Count one agent message on the link; rejects once the limit is reached. */
export async function recordLinkedMessage(
  from: string,
  to: string,
): Promise<LinkReservation> {
  const reservation = await invoke<LinkReservation>("session_link_record_message", {
    first: from,
    second: to,
    limit: LINK_MESSAGE_LIMIT,
  });
  void refreshSessionLinks();
  return reservation;
}

/**
 * Give back a counted message that the target session did not accept. The
 * backend ignores it if the user reset the budget since it was counted.
 */
export async function releaseLinkedMessage(
  from: string,
  to: string,
  reservation: LinkReservation,
) {
  await invoke("session_link_release_message", {
    first: from,
    second: to,
    epoch: reservation.epoch,
  });
  void refreshSessionLinks();
}

/** A user message in a linked session lets its agents message again. */
export function resetLinkBudget(sessionId: string) {
  if (!available()) return;
  // The link cache can be empty or stale here, so always ask the backend.
  // It only resets linked rows with a count and returns how many it changed.
  void invoke<number>("session_links_reset", { sessionId })
    .then((changed) => (changed > 0 ? refreshSessionLinks() : undefined))
    .catch(() => undefined);
}
