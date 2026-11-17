/** Where a Mono's overview was scrolled: its list or tab, and an open
 * document. Read while mounted, put back once it mounts again. */
export type MonoScroll = {
  overview?: { tab: string; top: number };
  document?: { top: number };
};

const OVERVIEW = '[data-mono-scroll="overview"]:not([hidden])';
const DOCUMENT = '[data-mono-scroll="document"]';

export function readMonoScroll(
  root: ParentNode = document,
): MonoScroll | undefined {
  const overview = root.querySelector<HTMLElement>(OVERVIEW);
  const page = root.querySelector<HTMLElement>(DOCUMENT);
  const scroll: MonoScroll = {};
  const tab = overview?.dataset.monoOverview;
  if (overview && tab && overview.scrollTop > 0) {
    scroll.overview = { tab, top: overview.scrollTop };
  }
  if (page && page.scrollTop > 0) scroll.document = { top: page.scrollTop };
  return scroll.overview || scroll.document ? scroll : undefined;
}

let cancelActive: (() => void) | undefined;

/** Stops a restore still waiting for its content. */
export function cancelMonoScrollRestore() {
  cancelActive?.();
}

type Options = {
  root?: Document;
  frame?: (callback: () => void) => number;
  cancelFrame?: (handle: number) => void;
  /** Frames to wait for content to load before settling for what fits. */
  frames?: number;
};

const INPUT = ["wheel", "touchstart", "keydown", "pointerdown"] as const;

/** Scrolls the remounted overview back once each scroller has room for it. A
 * document loads after it mounts, so this waits a bounded while. The reader
 * taking over the scroll, or another Mono opening, cancels it. */
export function restoreMonoScroll(
  scroll: MonoScroll,
  {
    root = document,
    frame = (callback) => requestAnimationFrame(callback),
    cancelFrame = (handle) => cancelAnimationFrame(handle),
    frames = 60,
  }: Options = {},
) {
  cancelMonoScrollRestore();
  const pending = [
    scroll.overview && {
      top: scroll.overview.top,
      find: () => {
        const el = root.querySelector<HTMLElement>(OVERVIEW);
        return el?.dataset.monoOverview === scroll.overview?.tab ? el : null;
      },
    },
    scroll.document && {
      top: scroll.document.top,
      find: () => root.querySelector<HTMLElement>(DOCUMENT),
    },
  ].filter((entry) => !!entry);
  let handle = 0;
  let stopped = false;
  const stop = () => {
    stopped = true;
    cancelFrame(handle);
    for (const type of INPUT) root.removeEventListener(type, stop, true);
    if (cancelActive === stop) cancelActive = undefined;
  };
  for (const type of INPUT) root.addEventListener(type, stop, true);
  cancelActive = stop;
  let waited = 0;
  const tick = () => {
    if (stopped) return;
    const last = ++waited >= frames;
    for (let index = pending.length - 1; index >= 0; index--) {
      const el = pending[index].find();
      if (
        el &&
        (last || el.scrollHeight - el.clientHeight >= pending[index].top - 1)
      ) {
        el.scrollTop = pending[index].top;
        pending.splice(index, 1);
      } else if (last) pending.splice(index, 1);
    }
    if (pending.length) handle = frame(tick);
    else stop();
  };
  handle = frame(tick);
}
