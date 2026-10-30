/**
 * Drawing the live view's pictures, and moving them ahead of the browser
 * while the user scrolls.
 *
 * The painter runs in a worker on an OffscreenCanvas, so decoding and
 * drawing never wait behind the transcript streaming on the main thread.
 * The same painter draws on the main thread where a webview has no
 * OffscreenCanvas.
 */

/** A picture for the painter: the packed frame's fields and its bytes. */
export type PaintFrame = {
  seq: number;
  format: "jpeg" | "png";
  /** The page's viewport in CSS pixels. */
  width: number;
  height: number;
  /** How far the page was scrolled, in CSS pixels. */
  scrollX: number;
  scrollY: number;
  picture: Uint8Array;
};

/** A picture now on screen, as the painter reports it. */
export type Drawn = {
  seq: number;
  width: number;
  height: number;
  scrollX: number;
  scrollY: number;
};

/** Where the page should be scrolled to soon, in CSS pixels. */
export type ScrollTarget = { x: number; y: number };

type Surface = HTMLCanvasElement | OffscreenCanvas;
type Context2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

/**
 * The newest picture, drawn moved by how far the page is about to scroll
 * past it: the user's wheel moves the page at once, and the browser's next
 * picture lands where the page already is. What the move uncovers gets the
 * picture's edge stretched over it, which on most pages is the background.
 */
export class LivePainter {
  private bitmap: ImageBitmap | null = null;
  private shown: Drawn | null = null;
  private newest = 0;
  private target: ScrollTarget | null = null;
  private context: Context2D | null = null;
  private closed = false;

  constructor(
    private readonly surface: Surface,
    private readonly drawn: (drawn: Drawn) => void,
  ) {}

  /** Decode and draw `frame`; one overtaken meanwhile is dropped. */
  frame(frame: PaintFrame): void {
    if (this.closed || frame.seq <= this.newest) return;
    this.newest = frame.seq;
    void createImageBitmap(
      new Blob([frame.picture as Uint8Array<ArrayBuffer>], {
        type: `image/${frame.format}`,
      }),
    )
      .then((bitmap) => {
        if (this.closed || frame.seq < this.newest) {
          bitmap.close();
          return;
        }
        this.bitmap?.close();
        this.bitmap = bitmap;
        this.shown = {
          seq: frame.seq,
          width: frame.width,
          height: frame.height,
          scrollX: frame.scrollX,
          scrollY: frame.scrollY,
        };
        this.draw();
        this.drawn(this.shown);
      })
      .catch(() => {});
  }

  /** Where the page is about to be scrolled to; `null` when nowhere. */
  scrollTo(target: ScrollTarget | null): void {
    const same =
      target === this.target ||
      (target &&
        this.target &&
        target.x === this.target.x &&
        target.y === this.target.y);
    this.target = target;
    if (!same) this.draw();
  }

  close(): void {
    this.closed = true;
    this.bitmap?.close();
    this.bitmap = null;
  }

  private draw(): void {
    const bitmap = this.bitmap;
    const shown = this.shown;
    if (!bitmap || !shown) return;
    const { width, height } = bitmap;
    if (this.surface.width !== width) this.surface.width = width;
    if (this.surface.height !== height) this.surface.height = height;
    this.context ??= this.surface.getContext("2d", {
      alpha: false,
    }) as Context2D | null;
    const context = this.context;
    if (!context) return;
    const ratio = shown.width > 0 ? width / shown.width : 1;
    const [dx, dy] = pictureShift(shown, this.target, ratio, width, height);
    if (dx === 0 && dy === 0) {
      context.drawImage(bitmap, 0, 0);
      return;
    }
    context.drawImage(bitmap, -dx, -dy);
    // The uncovered band takes the colour at the picture's edge, by its
    // side: on most pages the background.
    const edge = (
      sx: number,
      sy: number,
      x: number,
      y: number,
      w: number,
      h: number,
    ) => context.drawImage(bitmap, sx, sy, 1, 1, x, y, w, h);
    if (dy > 0) edge(1, height - 1, 0, height - dy, width, dy);
    else if (dy < 0) edge(1, 0, 0, 0, width, -dy);
    if (dx > 0) edge(width - 1, 1, width - dx, 0, dx, height);
    else if (dx < 0) edge(0, 1, 0, 0, -dx, height);
  }
}

/**
 * How far to move a picture, in its own whole pixels, so it shows the page
 * at `target`: never more than the picture itself.
 */
export function pictureShift(
  shown: Pick<Drawn, "scrollX" | "scrollY">,
  target: ScrollTarget | null,
  ratio: number,
  width: number,
  height: number,
): [number, number] {
  if (!target) return [0, 0];
  const clamp = (value: number, limit: number) =>
    Math.max(-limit, Math.min(limit, value));
  return [
    clamp(Math.round((target.x - shown.scrollX) * ratio), width),
    clamp(Math.round((target.y - shown.scrollY) * ratio), height),
  ];
}

/** How long after the last wheel a page that has not followed it is let be. */
export const SETTLE_MS = 160;
/** How close, in CSS pixels, the page counts as where the wheel sent it. */
const NEAR = 1.5;

/**
 * Where the page is about to be scrolled to, from the wheel the panel sends
 * and the pictures that come back.
 *
 * A wheel scrolls the page itself unless the pointer is over something that
 * scrolls of its own (a list, a code block, an app whose body never
 * scrolls); only the page's own scroll comes with the pictures. So the
 * picture is moved ahead only once this page has shown that the wheel
 * scrolls it, and a page that does not follow a wheel (a scroller of its
 * own, or the end of the page) is let be after {@link SETTLE_MS}.
 */
export class ScrollPredictor {
  /** The page's scroll in the newest picture. */
  private page: ScrollTarget = { x: 0, y: 0 };
  private target: ScrollTarget | null = null;
  /** The wheel is known to scroll this page. */
  private follows = false;
  /** Where the page was when wheels started going out unpredicted. */
  private probe: ScrollTarget | null = null;
  private lastWheel = -Infinity;
  private lastMove = -Infinity;
  /** Since when the picture has been ahead of the page. */
  private aheadSince = -Infinity;
  /** The viewport, which bounds how far ahead the picture goes. */
  private view = { width: 0, height: 0 };

  /** A wheel of `dx`, `dy` CSS pixels sent to the page at `now`. */
  wheel(dx: number, dy: number, now: number): ScrollTarget | null {
    this.lastWheel = now;
    // Wheels keep coming and the page has stopped following them (its end,
    // or a scroller of its own under the pointer).
    if (
      this.target &&
      now - this.aheadSince >= SETTLE_MS &&
      now - this.lastMove >= SETTLE_MS
    ) {
      this.target = null;
      this.follows = false;
    }
    if (!this.follows) {
      this.probe ??= { ...this.page };
      return null;
    }
    const base = this.target ?? this.page;
    // At most half a viewport ahead: the band a page that stops shows.
    const limit = (value: number, from: number, size: number) =>
      size > 0
        ? Math.max(from - size / 2, Math.min(from + size / 2, value))
        : value;
    if (!this.target) this.aheadSince = now;
    this.target = {
      x: limit(Math.max(0, base.x + dx), this.page.x, this.view.width),
      y: limit(Math.max(0, base.y + dy), this.page.y, this.view.height),
    };
    return this.target;
  }

  /** A picture is on screen: the page as it shows it. */
  seen(drawn: Drawn, now: number): ScrollTarget | null {
    const moved =
      drawn.scrollX !== this.page.x || drawn.scrollY !== this.page.y;
    this.page = { x: drawn.scrollX, y: drawn.scrollY };
    this.view = { width: drawn.width, height: drawn.height };
    if (moved) {
      this.lastMove = now;
      // The wheel moved the page itself: from now on it goes ahead.
      if (this.probe) {
        this.follows = true;
        this.probe = null;
      }
    }
    if (this.target && this.reached()) this.target = null;
    return this.settle(now);
  }

  /**
   * The page is where the wheel sent it, give or take the device pixel the
   * browser rounds a scroll to.
   */
  private reached(): boolean {
    const target = this.target;
    return (
      !target ||
      (Math.abs(target.x - this.page.x) <= NEAR &&
        Math.abs(target.y - this.page.y) <= NEAR)
    );
  }

  /**
   * At `now`, with no picture since. Once the wheel has stopped and the
   * page with it, the picture goes back to where the page is; a page that
   * never moved while the picture was ahead is not following the wheel,
   * and the next wheel finds out again.
   */
  settle(now: number): ScrollTarget | null {
    if (now - this.lastWheel < SETTLE_MS) return this.target;
    if (this.target && (this.reached() || now - this.lastMove >= SETTLE_MS)) {
      if (!this.reached() && this.lastMove < this.aheadSince)
        this.follows = false;
      this.target = null;
    }
    if (!this.target) this.probe = null;
    return this.target;
  }

  /** A page scrolled before (by the agent, by the user) scrolls itself. */
  learn(drawn: Drawn): void {
    if (drawn.scrollX > 0 || drawn.scrollY > 0) this.follows = true;
  }

  /** Another page: nothing is known about it. */
  reset(): void {
    this.target = null;
    this.follows = false;
    this.probe = null;
  }
}
