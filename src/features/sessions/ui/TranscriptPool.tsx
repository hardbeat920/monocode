import {
  cloneElement,
  memo,
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
  type ReactElement,
} from "react";
import { createPortal } from "react-dom";
import { estimateRetainedBytes } from "../data/sessionCache";
import { detachedHistoryNodes } from "./useMeasuredHistory";

/** Transcripts kept mounted after their pane closes, so a revisit skips the rebuild. */
export const TRANSCRIPT_POOL_LIMIT = 4;
// Each retained node also holds its React fiber and render objects, roughly
// 1-2 KB together, so the node budget is the pool's real memory ceiling.
// Four large transcripts need about 50k nodes: at 40k, revisits rebuild and
// take twice as long. Workspace tabs stay mounted and do not count here;
// releasing them measured slower and no smaller with real sessions.
// Sustained pressure reduces only parked entries; visible content stays intact.
export const TRANSCRIPT_POOL_NODE_LIMIT = 50_000;
const PRESSURED_NODE_LIMIT = 10_000;
export const TRANSCRIPT_POOL_DATA_LIMIT = 8 * 1024 * 1024;

type PooledProps = { visible?: boolean; parked?: boolean; blocks?: object[] };

export type TranscriptPoolEntry = {
  id: string;
  container: HTMLDivElement;
  element: ReactElement<PooledProps>;
  onMouseDown?: () => void;
  host: HTMLElement | null;
  retainedNodes: number;
  retainedBytes: number;
};

/**
 * Owns one mounted transcript per recently shown session. A pane lends it a
 * host element; the transcript renders into a container that moves between
 * hosts, so switching back to a session reuses its parsed, laid-out DOM
 * instead of mounting every turn and markdown block again.
 */
export class TranscriptPool {
  private entries = new Map<string, TranscriptPoolEntry>();
  private listeners = new Set<() => void>();
  private snapshot: TranscriptPoolEntry[] = [];
  private pressured = false;
  // `entries` is kept least-recently-shown first for eviction. The outlet
  // renders in arrival order instead: reordering would move a transcript's
  // portal on every switch, and React treats a moved component as newly
  // placed (StrictMode replays every effect in it).
  private arrival = new Map<string, number>();
  private nextArrival = 0;

  private get nodeBudget() {
    return this.pressured
      ? Math.min(this.nodeLimit, PRESSURED_NODE_LIMIT)
      : this.nodeLimit;
  }

  setResourcePressure = (pressured: boolean) => {
    if (this.pressured === pressured) return;
    this.pressured = pressured;
    this.trim();
    this.emit();
  };

  constructor(
    private readonly limit = TRANSCRIPT_POOL_LIMIT,
    private readonly nodeLimit = TRANSCRIPT_POOL_NODE_LIMIT,
    private readonly dataLimit = TRANSCRIPT_POOL_DATA_LIMIT,
  ) {}

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = () => this.snapshot;

  show(
    id: string,
    host: HTMLElement,
    element: ReactElement<PooledProps>,
    onMouseDown?: () => void,
  ) {
    const previous = this.entries.get(id);
    const container = previous?.container ?? createContainer();
    // Attach before the pane's passive effects run, so anything that looks
    // for the transcript in the pane already finds a revisited one.
    if (element.props.visible === false) container.remove();
    else if (container.parentElement !== host) host.appendChild(container);
    if (
      previous?.host === host &&
      previous.onMouseDown === onMouseDown &&
      sameElement(previous.element, element)
    )
      return;
    // Re-inserting keeps the map in least-recently-shown order.
    this.entries.delete(id);
    this.entries.set(id, {
      id,
      container,
      element,
      onMouseDown,
      host,
      retainedNodes: 0,
      retainedBytes: 0,
    });
    this.emit();
  }

  park(id: string, host: HTMLElement) {
    const entry = this.entries.get(id);
    if (!entry || entry.host !== host) return;
    entry.container.remove();
    const parked = {
      ...entry,
      host: null,
      onMouseDown: undefined,
      retainedNodes:
        countNodes(entry.container, this.nodeBudget) +
        detachedHistoryNodes(entry.container),
      retainedBytes: entry.element.props.blocks
        ? estimateRetainedBytes(entry.element.props.blocks)
        : 0,
      element: cloneElement(entry.element, { visible: false, parked: true }),
    };
    if (
      parked.retainedNodes > this.nodeBudget ||
      parked.retainedBytes > this.dataLimit
    ) {
      // One oversized conversation must not evict all the inexpensive ones.
      this.entries.delete(id);
    } else this.entries.set(id, parked);
    this.trim();
    this.emit();
  }

  private trim() {
    let parked = 0;
    let nodes = 0;
    let bytes = 0;
    for (const entry of this.entries.values()) {
      if (entry.host) continue;
      parked += 1;
      nodes += entry.retainedNodes;
      bytes += entry.retainedBytes;
    }
    for (const entry of [...this.entries.values()]) {
      if (
        parked <= this.limit &&
        nodes <= this.nodeBudget &&
        bytes <= this.dataLimit
      )
        break;
      if (entry.host) continue;
      this.entries.delete(entry.id);
      parked -= 1;
      nodes -= entry.retainedNodes;
      bytes -= entry.retainedBytes;
    }
  }

  private emit() {
    for (const id of this.arrival.keys())
      if (!this.entries.has(id)) this.arrival.delete(id);
    for (const id of this.entries.keys())
      if (!this.arrival.has(id)) this.arrival.set(id, this.nextArrival++);
    this.snapshot = [...this.entries.values()].sort(
      (a, b) => this.arrival.get(a.id)! - this.arrival.get(b.id)!,
    );
    for (const listener of this.listeners) listener();
  }
}

/** Bounded traversal, including text nodes; no layout or serialized DOM copy. */
function countNodes(container: HTMLElement, limit: number): number {
  const walker = document.createTreeWalker(container);
  let count = 0;
  while (count <= limit && walker.nextNode()) count++;
  return count;
}

function sameElement(
  previous: ReactElement<PooledProps>,
  next: ReactElement<PooledProps>,
) {
  if (previous === next) return true;
  if (previous.type !== next.type || previous.key !== next.key) return false;
  const before = previous.props as Record<string, unknown>;
  const after = next.props as Record<string, unknown>;
  const keys = Object.keys(before);
  return (
    keys.length === Object.keys(after).length &&
    keys.every(
      (key) =>
        Object.prototype.hasOwnProperty.call(after, key) &&
        Object.is(before[key], after[key]),
    )
  );
}

function createContainer() {
  const node = document.createElement("div");
  node.className = "contents";
  return node;
}

/** Renders every pooled transcript into its current container. */
export const TranscriptPoolOutlet = memo(function TranscriptPoolOutlet({
  pool,
}: {
  pool: TranscriptPool;
}) {
  const entries = useSyncExternalStore(
    pool.subscribe,
    pool.getSnapshot,
    pool.getSnapshot,
  );
  return entries.map((entry) => <PooledEntry key={entry.id} entry={entry} />);
});

const PooledEntry = memo(function PooledEntry({
  entry,
}: {
  entry: TranscriptPoolEntry;
}) {
  // React events bubble through the portal's owner, not the pane. Forward the
  // one the pane listens for so clicking the transcript still focuses it.
  return createPortal(
    <div className="contents" onMouseDown={entry.onMouseDown}>
      {entry.element}
    </div>,
    entry.container,
  );
});

/**
 * Where a pane shows its transcript. Without a pool (tests, the inbox, which
 * keeps its panes mounted) the transcript renders in place as before.
 */
export function PooledTranscript({
  pool,
  sessionId,
  onMouseDown,
  children,
}: {
  pool?: TranscriptPool;
  sessionId: string;
  onMouseDown?: () => void;
  children: ReactElement<PooledProps>;
}) {
  const host = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const node = host.current;
    if (!pool || !node) return;
    return () => pool.park(sessionId, node);
  }, [pool, sessionId]);

  useLayoutEffect(() => {
    if (pool && host.current) {
      pool.show(sessionId, host.current, children, onMouseDown);
    }
  });

  if (!pool) return children;
  return <div ref={host} className="contents" />;
}
