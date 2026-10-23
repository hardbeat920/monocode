import type { InboxProvider } from "./githubTasks";

const DISABLED_KEY = "monocode.inboxDisabledConnectors";

/**
 * Announced when connector switches change, so the Inbox can refetch and the
 * rail can refresh without waiting for its poll.
 */
export const INBOX_CONNECTORS_CHANGE_EVENT =
  "monocode:inbox-connectors-changed";

const CONNECTOR_IDS: InboxProvider[] = [
  "github",
  "linear",
  "jira",
  "gitlab",
  "azuredevops",
];

function isConnectorId(value: string): value is InboxProvider {
  return (CONNECTOR_IDS as string[]).includes(value);
}

function readDisabled(): InboxProvider[] {
  try {
    const raw = localStorage.getItem(DISABLED_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (value): value is InboxProvider =>
        typeof value === "string" && isConnectorId(value),
    );
  } catch {
    return [];
  }
}

/**
 * Absent storage means every connector is enabled, so an existing
 * installation keeps all five until the user flips a switch.
 */
export function loadInboxDisabledConnectors(): InboxProvider[] {
  return readDisabled();
}

export function inboxConnectorEnabled(source: InboxProvider): boolean {
  return !readDisabled().includes(source);
}

export function saveInboxDisabledConnectors(
  disabled: InboxProvider[],
): boolean {
  const seen = new Set<string>();
  const clean = disabled.filter((value) => {
    if (!isConnectorId(value) || seen.has(value)) return false;
    seen.add(value);
    return true;
  });
  try {
    if (clean.length > 0) {
      localStorage.setItem(DISABLED_KEY, JSON.stringify(clean));
    } else {
      localStorage.removeItem(DISABLED_KEY);
    }
    notifyInboxConnectorsChanged();
    return true;
  } catch {
    return false;
  }
}

export function setInboxConnectorEnabled(
  source: InboxProvider,
  enabled: boolean,
): void {
  const disabled = readDisabled();
  const next = enabled
    ? disabled.filter((item) => item !== source)
    : disabled.includes(source)
      ? disabled
      : [...disabled, source];
  saveInboxDisabledConnectors(next);
}

export function notifyInboxConnectorsChanged(): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(INBOX_CONNECTORS_CHANGE_EVENT));
  }
}
