/** Shared with the list's insert motion so drag, drop and insert feel alike. */
const EASE = "cubic-bezier(0.32, 0.72, 0, 1)";
const LIFT_MS = 160;
const SETTLE_MS = 320;
const REFLOW_MS = 300;
/** How far the lifted card grows and tilts while it is carried. */
const LIFT_SCALE = 1.03;
const LIFT_TILT_DEG = -1.5;

function reducedMotion(): boolean {
  return !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
}

function canAnimate(el: Element): el is HTMLElement {
  return el instanceof HTMLElement && typeof el.animate === "function";
}

export type SessionDragGhost = {
  /** Keep the lifted card under the pointer. */
  move(x: number, y: number): void;
  /** Fly into `target`'s box (the dropped card's new home), then disappear. */
  land(target: HTMLElement | null): void;
  /** Return to where the card was picked up, then disappear. */
  cancel(): void;
};

/**
 * A copy of `card` lifted off the list that follows the pointer. The card
 * itself stays in place as a faded placeholder. Ghost markup is inert: it
 * ignores the pointer, so hit-testing for drop targets sees through it.
 */
export function liftSessionGhost(
  card: HTMLElement,
  x: number,
  y: number,
): SessionDragGhost {
  const origin = card.getBoundingClientRect();
  const grabX = x - origin.left;
  const grabY = y - origin.top;
  const ghost = card.cloneNode(true) as HTMLElement;
  for (const el of [ghost, ...ghost.querySelectorAll("*")]) {
    for (const name of el.getAttributeNames()) {
      if (
        name === "id" ||
        name === "role" ||
        name === "tabindex" ||
        name.startsWith("data-") ||
        name.startsWith("aria-")
      )
        el.removeAttribute(name);
    }
  }
  ghost.setAttribute("aria-hidden", "true");
  ghost.classList.remove("opacity-40");
  Object.assign(ghost.style, {
    position: "fixed",
    left: `${origin.left}px`,
    top: `${origin.top}px`,
    width: `${origin.width}px`,
    height: `${origin.height}px`,
    margin: "0",
    zIndex: "9999",
    pointerEvents: "none",
    boxSizing: "border-box",
    background:
      "color-mix(in srgb, var(--color-background-base) 92%, var(--color-content))",
    borderColor: "color-mix(in srgb, var(--color-accent) 45%, transparent)",
    boxShadow:
      "0 10px 28px -6px rgb(0 0 0 / 0.45), 0 2px 6px rgb(0 0 0 / 0.25)",
    transformOrigin: `${grabX}px ${grabY}px`,
    willChange: "transform",
  } satisfies Partial<CSSStyleDeclaration>);
  document.body.appendChild(ghost);

  const still = reducedMotion();
  const lifted = still
    ? "none"
    : `scale(${LIFT_SCALE}) rotate(${LIFT_TILT_DEG}deg)`;
  let dx = 0;
  let dy = 0;
  const place = () => {
    ghost.style.transform = `translate(${dx}px, ${dy}px) ${lifted}`;
  };
  place();
  if (!still && canAnimate(ghost)) {
    ghost.animate(
      [
        { transform: "translate(0, 0)", boxShadow: "0 0 0 rgb(0 0 0 / 0)" },
        { transform: ghost.style.transform, boxShadow: ghost.style.boxShadow },
      ],
      { duration: LIFT_MS, easing: EASE },
    );
  }

  let done = false;
  const flyTo = (rect: DOMRect | null, fade: boolean) => {
    if (done) return;
    done = true;
    if (still || !rect || !canAnimate(ghost)) {
      ghost.remove();
      return;
    }
    const to = `translate(${rect.left - origin.left}px, ${rect.top - origin.top}px)`;
    const flight = ghost.animate(
      [
        { transform: ghost.style.transform, opacity: 1 },
        {
          transform: to,
          opacity: fade ? 0 : 1,
          boxShadow: "0 0 0 rgb(0 0 0 / 0)",
        },
      ],
      { duration: SETTLE_MS, easing: EASE, fill: "forwards" },
    );
    const remove = () => ghost.remove();
    flight.finished.then(remove, remove);
    // Backstop in case the animation never reports back (e.g. a hidden tab).
    window.setTimeout(remove, SETTLE_MS + 100);
  };

  return {
    move(nextX, nextY) {
      if (done) return;
      dx = nextX - grabX - origin.left;
      dy = nextY - grabY - origin.top;
      place();
    },
    land(target) {
      if (!target) {
        flyTo(null, true);
        return;
      }
      // The real card is drawn in place underneath; hide it until the ghost
      // arrives so the drop reads as one card moving, not two.
      const reveal = target.style.opacity;
      target.style.opacity = "0";
      flyTo(target.getBoundingClientRect(), false);
      window.setTimeout(
        () => {
          target.style.opacity = reveal;
        },
        still ? 0 : SETTLE_MS,
      );
    },
    cancel() {
      flyTo(origin, false);
    },
  };
}

type ListLayout = Map<string, DOMRect>;

function listRows(list: HTMLElement): Map<string, HTMLElement> {
  const rows = new Map<string, HTMLElement>();
  for (const el of list.querySelectorAll<HTMLElement>(
    "[data-session-card],[data-session-folder]",
  )) {
    const key = el.dataset.sessionCard
      ? `card:${el.dataset.sessionCard}`
      : `folder:${el.dataset.sessionFolder}`;
    rows.set(key, el);
  }
  return rows;
}

/** Where every card and folder sits now, to animate from after a change. */
export function captureSessionListLayout(list: HTMLElement | null): ListLayout {
  const layout: ListLayout = new Map();
  if (!list) return layout;
  for (const [key, el] of listRows(list)) {
    layout.set(key, el.getBoundingClientRect());
  }
  return layout;
}

/**
 * Slide rows from where `before` saw them to where they are now (FLIP), so
 * a regrouping reads as rows making room rather than the list jumping.
 * `skip` is the dropped card, which the ghost brings in instead.
 */
export function playSessionListLayout(
  list: HTMLElement | null,
  before: ListLayout,
  skip?: string,
) {
  if (!list || reducedMotion()) return;
  const rows = listRows(list);
  const shifts = new Map<HTMLElement, { dx: number; dy: number }>();
  for (const [key, el] of rows) {
    const was = before.get(key);
    if (!was) continue;
    const now = el.getBoundingClientRect();
    shifts.set(el, { dx: was.left - now.left, dy: was.top - now.top });
  }
  for (const [key, el] of rows) {
    if (key === `card:${skip}` || !canAnimate(el)) continue;
    const shift = shifts.get(el);
    if (!shift) {
      // New here, such as a folder made by this drop: grow in.
      el.animate(
        [
          { opacity: 0, transform: "scale(0.97)" },
          { opacity: 1, transform: "none" },
        ],
        { duration: REFLOW_MS, easing: EASE },
      );
      continue;
    }
    // A card inside a folder already rides along with the folder's own
    // slide; it only needs to move by what is left over.
    const folder = el.dataset.sessionCard
      ? el.parentElement?.closest<HTMLElement>("[data-session-folder]")
      : null;
    const carried = folder ? shifts.get(folder) : undefined;
    const dx = shift.dx - (carried?.dx ?? 0);
    const dy = shift.dy - (carried?.dy ?? 0);
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    el.animate(
      [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }],
      { duration: REFLOW_MS, easing: EASE },
    );
  }
}
