/**
 * How the phone's copy of a draft page follows the desktop's record, and what
 * of the person's use of it goes back. Only the page's `mc.state` and the pins
 * are shared; everything else a page keeps stays on this screen.
 */

/** The element a pin was placed on, so it lands there at any width. */
export type DraftAnchor = { path: string; ax: number; ay: number };
export type DraftPin = { x: number; y: number; side: "desktop" | "phone"; at?: DraftAnchor };
export type DraftNote = {
  note: string;
  tag: string;
  name: string;
  x: number;
  y: number;
  at?: DraftAnchor;
};
export type DraftState = Record<string, unknown>;

/** A draft whose page is not open on the desktop, as a `peek` says. */
export type ShutDraft = { live: false; saved?: boolean; dismissed?: boolean };

/** The desktop's record of a draft, as `draft.read` and `draft.action` answer. */
export type DraftRecord =
  | ShutDraft
  | {
      live: true;
      title: string;
      rev: number;
      generation: number;
      kind: "html" | "url";
      state: DraftState | null;
      stateRev: number;
      pins: DraftPin[];
      pending: DraftNote[];
      control: boolean;
      waiting: boolean;
      /** Present when the phone does not have this generation's page yet. */
      page?: string | null;
      /** A dev server's address; the phone gets pictures of it, not the page. */
      url?: string;
      /** How tall the page was at a phone's width, before the page says itself. */
      height?: number;
    };
export type LiveDraft = Extract<DraftRecord, { live: true }>;

/** What the page says to the app through its bridge. */
export type PageMessage =
  | { k: "v"; v: DraftState }
  | { k: "f"; f: DraftNote }
  | { k: "u"; a?: string }
  | { k: "n"; t: string }
  | { k: "h"; v: number };

/** What the page needs to be sent, by `__mcSend`. */
export type PageCommand =
  | { __mcState: DraftState }
  | { __mcPin: { x: number; y: number; at?: DraftAnchor } }
  | { __mcComment: boolean };

/** What goes to the desktop as a `draft.action`. */
export type DraftAction =
  | { action: "state"; values: DraftState }
  | { action: "pin"; notes: DraftNote[] }
  | { action: "input"; actions: string[] }
  | { action: "message"; text: string };

/** How recently the person must have touched the page for it to speak for them. */
export const TOUCH_WINDOW_MS = 2000;
/** A note is typed after the tap that opened it; it may take this long. */
export const NOTE_WINDOW_MS = 120_000;
/** How long the page may take to report values it was just given. */
export const PUSH_GRACE_MS = 1000;

export type PageSync = {
  /** The shared state revision the page has; 0 until its first report this load. */
  stateRev: number;
  /** What the page last reported. */
  reported: DraftState | null;
  pushed: { values: DraftState; at: number } | null;
  /** How many of the record's pins the page draws. */
  pins: number;
  /** Whether the page loaded since the pins were last drawn, so its own are gone. */
  fresh: boolean;
  /** State changes sent to the desktop and not answered yet. Until they are,
   *  the page keeps what the person is doing rather than an older answer. */
  sending: number;
};

export const freshSync = (): PageSync => ({
  stateRev: 0,
  reported: null,
  pushed: null,
  pins: 0,
  fresh: true,
  sending: 0,
});

export function sameState(a: DraftState | null | undefined, b: DraftState | null | undefined) {
  if (!a || !b) return (a ?? null) === (b ?? null);
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every((key) => key in b && JSON.stringify(a[key]) === JSON.stringify(b[key]));
}

/** The keys of `next` that differ from `base`. */
export function statePatch(base: DraftState, next: DraftState): DraftState {
  const patch: DraftState = {};
  for (const [key, value] of Object.entries(next))
    if (key in base && JSON.stringify(base[key]) !== JSON.stringify(value)) patch[key] = value;
  return patch;
}

/** What the page needs after the record changed: other screens' state and pins. */
export function follow(sync: PageSync, record: LiveDraft, now: number): PageCommand[] {
  const out: PageCommand[] = [];
  for (const pin of record.pins.slice(Math.min(sync.pins, record.pins.length)))
    if (sync.fresh || pin.side !== "phone")
      out.push({ __mcPin: pin.at ? { x: pin.x, y: pin.y, at: pin.at } : { x: pin.x, y: pin.y } });
  sync.pins = record.pins.length;
  sync.fresh = false;
  // Until the page's first report, it is still loading its own values.
  if (sync.stateRev !== 0 && sync.sending === 0 && sync.stateRev !== record.stateRev) {
    if (record.state && !sameState(record.state, sync.reported)) {
      out.push({ __mcState: record.state });
      sync.pushed = { values: record.state, at: now };
    }
    sync.stateRev = record.stateRev;
  }
  return out;
}

/**
 * The page reported its state. Returns what to tell the page and what to send
 * the desktop. A change counts as the person's only right after they touched
 * the page; one the page makes by itself stays on this screen.
 */
export function reportState(
  sync: PageSync,
  record: LiveDraft,
  values: DraftState,
  touchedAt: number,
  now: number,
): { page: PageCommand[]; desktop: DraftAction | null } {
  sync.reported = values;
  if (sync.pushed) {
    if (!sameState(values, sync.pushed.values) && now - sync.pushed.at < PUSH_GRACE_MS)
      return { page: [], desktop: null };
    sync.pushed = null;
  }
  if (sync.stateRev === 0) {
    // The page's starting values: the record keeps what was chosen elsewhere.
    sync.stateRev = record.stateRev;
    if (record.state && !sameState(record.state, values)) {
      sync.pushed = { values: record.state, at: now };
      return { page: [{ __mcState: record.state }], desktop: null };
    }
    return { page: [], desktop: null };
  }
  if (sameState(record.state, values) || now - touchedAt > TOUCH_WINDOW_MS)
    return { page: [], desktop: null };
  // The desktop's page has not run, so this page's values start the record.
  if (!record.state) return { page: [], desktop: { action: "state", values } };
  const patch = statePatch(record.state, values);
  if (!Object.keys(patch).length) return { page: [], desktop: null };
  return { page: [], desktop: { action: "state", values: patch } };
}

/**
 * Whether a message from the page may speak for the person: only right after
 * their own touch, and a note only while comment mode is on.
 */
export function trusted(
  message: PageMessage,
  touchedAt: number,
  commenting: boolean,
  now: number,
): boolean {
  if (message.k === "f") return commenting && now - touchedAt <= NOTE_WINDOW_MS;
  if (message.k === "u" || message.k === "n") return now - touchedAt <= TOUCH_WINDOW_MS;
  return true;
}

/** Reads one message from the page's bridge, or null for anything else. */
export function parseMessage(data: string): PageMessage | null {
  let value: unknown;
  try {
    value = JSON.parse(data);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object") return null;
  const message = value as Record<string, unknown>;
  if (message.__mc !== 1) return null;
  switch (message.k) {
    case "v":
      return message.v && typeof message.v === "object" && !Array.isArray(message.v)
        ? { k: "v", v: message.v as DraftState }
        : null;
    case "f": {
      const f = message.f as Partial<DraftNote> | undefined;
      return f && typeof f.note === "string" && Number.isFinite(f.x) && Number.isFinite(f.y)
        ? {
            k: "f",
            f: {
              note: f.note,
              tag: String(f.tag ?? ""),
              name: String(f.name ?? ""),
              x: f.x as number,
              y: f.y as number,
              ...anchorOf(f.at),
            },
          }
        : null;
    }
    case "u":
      return { k: "u", a: typeof message.a === "string" ? message.a : undefined };
    case "n":
      return typeof message.t === "string" ? { k: "n", t: message.t } : null;
    case "h":
      return Number.isFinite(message.v) ? { k: "h", v: message.v as number } : null;
    default:
      return null;
  }
}

/** A pin's element as the page reported it, or nothing for anything else. */
function anchorOf(value: unknown): { at?: DraftAnchor } {
  if (!value || typeof value !== "object") return {};
  const { path, ax, ay } = value as Partial<DraftAnchor>;
  const named = typeof path === "string" && path.length <= 160 && /^\d{1,4}(\.\d{1,4})*$/.test(path);
  return named && Number.isFinite(ax) && Number.isFinite(ay)
    ? { at: { path, ax: ax as number, ay: ay as number } }
    : {};
}

/** The newer of two answers about the same draft; an older one never wins. */
export function newer(current: DraftRecord | null, next: DraftRecord): DraftRecord {
  if (!current || !current.live || !next.live) return next;
  if (next.generation !== current.generation)
    return next.generation > current.generation ? next : current;
  return next.rev >= current.rev ? { ...next, page: next.page ?? current.page } : current;
}

/** The script that hands one command to the page. */
export const sendScript = (command: PageCommand) =>
  `window.__mcSend&&window.__mcSend(${JSON.stringify(command)});true;`;
