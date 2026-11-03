import { useLayoutEffect, useRef } from "react";

/** Room around the text for particles drifting past its box. */
const PAD = 20;
/** Sampling step in CSS pixels; one particle candidate per cell. */
const STEP = 1.5;
/** Share of sampled cells that become particles, so the burst stays light. */
const DENSITY = 0.9;
/** Time for the reveal edge to cross the title. */
const SWEEP_MS = 1100;
/** Softness of the reveal edge. */
const EDGE = 18;
const PARTICLE_MS = 850;

type Particle = {
  x: number;
  y: number;
  alpha: number;
  vx: number;
  vy: number;
  life: number;
};

function reducedMotion(): boolean {
  return !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
}

function fitText(ctx: CanvasRenderingContext2D, text: string, width: number) {
  if (ctx.measureText(text).width <= width) return text;
  let end = text.length;
  while (end > 0 && ctx.measureText(`${text.slice(0, end)}…`).width > width) {
    end -= 1;
  }
  return `${text.slice(0, end).trimEnd()}…`;
}

/** Rasterize `text` as `el` lays it out and scatter particles over its glyphs. */
function sampleText(
  text: string,
  el: HTMLElement,
  width: number,
  height: number,
): Particle[] {
  const style = getComputedStyle(el);
  const scale = Math.max(1, window.devicePixelRatio || 1);
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(width * scale);
  canvas.height = Math.ceil(height * scale);
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return [];
  ctx.scale(scale, scale);
  ctx.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
  // CSS centers the font's content box in the line box.
  ctx.textBaseline = "alphabetic";
  const fitted = fitText(ctx, text, width);
  const metrics = ctx.measureText(fitted);
  const ascent = metrics.fontBoundingBoxAscent;
  const descent = metrics.fontBoundingBoxDescent;
  const baseline =
    Number.isFinite(ascent) && Number.isFinite(descent)
      ? (height - (ascent + descent)) / 2 + ascent
      : height * 0.75;
  ctx.fillStyle = "#000";
  ctx.fillText(fitted, 0, baseline);

  const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const particles: Particle[] = [];
  for (let y = 0; y < height; y += STEP) {
    for (let x = 0; x < width; x += STEP) {
      const px = Math.floor(x * scale);
      const py = Math.floor(y * scale);
      const alpha = data[(py * canvas.width + px) * 4 + 3] / 255;
      if (alpha < 0.25 || Math.random() > DENSITY) continue;
      // A small, mostly forward and upward puff.
      const angle = -Math.PI / 2 + (Math.random() - 0.3) * Math.PI;
      const speed = 6 + Math.random() * 12;
      particles.push({
        x,
        y,
        alpha,
        vx: Math.cos(angle) * speed + 4,
        vy: Math.sin(angle) * speed * 0.8,
        life: PARTICLE_MS * (0.7 + Math.random() * 0.5),
      });
    }
  }
  return particles;
}

const easeOut = (t: number) => 1 - (1 - t) ** 3;
const easeInOut = (t: number) =>
  t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2;
const clamp01 = (t: number) => Math.min(1, Math.max(0, t));

function setMask(el: HTMLElement, mask: string) {
  el.style.maskImage = mask;
  el.style.setProperty("-webkit-mask-image", mask);
}

/**
 * Text that, when it changes, sweeps the new value in from left to right
 * while the old one bursts into particles at the edge. The first value
 * renders without motion.
 */
export function ParticleText({
  text,
  className = "",
}: {
  text: string;
  className?: string;
}) {
  const rootRef = useRef<HTMLSpanElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const shownRef = useRef(text);
  const stopRef = useRef<(() => void) | null>(null);

  useLayoutEffect(() => {
    const previous = shownRef.current;
    shownRef.current = text;
    if (previous === text) return;
    stopRef.current?.();
    const root = rootRef.current;
    const label = textRef.current;
    if (!root || !label || reducedMotion()) return;
    const width = label.clientWidth;
    const height = label.clientHeight;
    if (width === 0 || height === 0) return;

    const scale = Math.max(1, window.devicePixelRatio || 1);
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const particles = sampleText(previous, label, width, height);

    const rootBox = root.getBoundingClientRect();
    const labelBox = label.getBoundingClientRect();
    const left = labelBox.left - rootBox.left;
    const top = labelBox.top - rootBox.top;

    // The old title stays real text until the edge passes it.
    const ghost = document.createElement("span");
    ghost.className = label.className;
    ghost.textContent = previous;
    ghost.setAttribute("aria-hidden", "true");
    Object.assign(ghost.style, {
      position: "absolute",
      left: `${left}px`,
      top: `${top}px`,
      width: `${width}px`,
      pointerEvents: "none",
    });

    const canvasWidth = width + PAD * 2;
    const canvasHeight = height + PAD * 2;
    canvas.width = Math.ceil(canvasWidth * scale);
    canvas.height = Math.ceil(canvasHeight * scale);
    Object.assign(canvas.style, {
      position: "absolute",
      left: `${left - PAD}px`,
      top: `${top - PAD}px`,
      width: `${canvasWidth}px`,
      height: `${canvasHeight}px`,
      pointerEvents: "none",
    });
    canvas.setAttribute("aria-hidden", "true");
    ctx.scale(scale, scale);
    const color = getComputedStyle(label).color;
    root.append(ghost, canvas);

    const travel = width + EDGE * 2;
    const edgeAt = (elapsed: number) =>
      -EDGE + travel * easeInOut(clamp01(elapsed / SWEEP_MS));
    const reveal = (edge: number) => {
      setMask(
        label,
        `linear-gradient(90deg, #000 ${edge - EDGE}px, transparent ${edge}px)`,
      );
      setMask(
        ghost,
        `linear-gradient(90deg, transparent ${edge - EDGE / 2}px, #000 ${edge + EDGE / 2}px)`,
      );
    };
    reveal(-EDGE);

    // Each particle leaves when the edge reaches it.
    const born = particles.map(() => -1);
    const start = performance.now();
    let frame = 0;
    const stop = () => {
      cancelAnimationFrame(frame);
      canvas.remove();
      ghost.remove();
      setMask(label, "");
      stopRef.current = null;
    };
    stopRef.current = stop;

    const draw = (now: number) => {
      const elapsed = now - start;
      const edge = edgeAt(elapsed);
      reveal(edge);
      ctx.clearRect(0, 0, canvasWidth, canvasHeight);
      ctx.fillStyle = color;
      let alive = false;
      particles.forEach((p, index) => {
        if (born[index] < 0) {
          if (p.x > edge) {
            alive = true;
            return;
          }
          born[index] = elapsed;
        }
        const t = clamp01((elapsed - born[index]) / p.life);
        if (t >= 1) return;
        alive = true;
        const drift = easeOut(t);
        ctx.globalAlpha = Math.min(1, p.alpha * 1.2) * (1 - t) ** 1.2;
        const size = 1.8 * (1 - t * 0.5);
        ctx.fillRect(
          PAD + p.x + p.vx * drift,
          PAD + p.y + p.vy * drift,
          size,
          size,
        );
      });
      ctx.globalAlpha = 1;
      if (alive || elapsed < SWEEP_MS) {
        frame = requestAnimationFrame(draw);
      } else {
        stop();
      }
    };
    frame = requestAnimationFrame(draw);
  }, [text]);

  useLayoutEffect(() => () => stopRef.current?.(), []);

  return (
    <span ref={rootRef} className="relative flex min-w-0 flex-1">
      <span ref={textRef} className={`min-w-0 flex-1 ${className}`.trim()}>
        {text}
      </span>
    </span>
  );
}
