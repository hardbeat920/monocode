/**
 * Drawing the live view's pictures.
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
  picture: Uint8Array;
};

/** A picture now on screen, as the painter reports it. */
export type Drawn = {
  seq: number;
  width: number;
  height: number;
};

type Surface = HTMLCanvasElement | OffscreenCanvas;
type Context2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

/**
 * The newest picture, as the browser made it. Moving it ahead of the page
 * while the user scrolls was tried: the band it uncovered has no picture
 * yet, and on a fast scroll that read as half the page vanishing.
 */
export class LivePainter {
  private bitmap: ImageBitmap | null = null;
  private newest = 0;
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
        this.draw();
        this.drawn({
          seq: frame.seq,
          width: frame.width,
          height: frame.height,
        });
      })
      .catch(() => {});
  }

  close(): void {
    this.closed = true;
    this.bitmap?.close();
    this.bitmap = null;
  }

  private draw(): void {
    const bitmap = this.bitmap;
    if (!bitmap) return;
    const { width, height } = bitmap;
    if (this.surface.width !== width) this.surface.width = width;
    if (this.surface.height !== height) this.surface.height = height;
    this.context ??= this.surface.getContext("2d", {
      alpha: false,
    }) as Context2D | null;
    this.context?.drawImage(bitmap, 0, 0);
  }
}
