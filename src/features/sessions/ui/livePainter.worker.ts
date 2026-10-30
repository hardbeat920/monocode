import { type PaintFrame, LivePainter, type ScrollTarget } from "./livePainter";

/** What the live view sends its painter (see `BrowserLiveView`). */
export type PainterMessage =
  | { kind: "canvas"; canvas: OffscreenCanvas }
  | ({ kind: "frame" } & PaintFrame)
  | { kind: "scroll"; target: ScrollTarget | null }
  | { kind: "close" };

let painter: LivePainter | null = null;

self.onmessage = ({ data }: MessageEvent<PainterMessage>) => {
  switch (data.kind) {
    case "canvas":
      painter = new LivePainter(data.canvas, (drawn) =>
        self.postMessage(drawn),
      );
      return;
    case "frame":
      painter?.frame(data);
      return;
    case "scroll":
      painter?.scrollTo(data.target);
      return;
    case "close":
      painter?.close();
      painter = null;
      self.close();
  }
};
