import { useCallback, useEffect, useRef, useState } from "react";
import { HARNESS_ICONS, MONOCHROME_HARNESSES } from "../chrome/HarnessIcon";
import { MASCOT_GRID, PROJECT_MASCOTS } from "../lib/projectMascots";
import { HARNESSES, type HarnessId } from "../lib/session";
import {
  ARCADE_MODES,
  type ArcadeMode,
  type ArcadeSprite,
  type GridArcade,
} from "./gridArcade";
import {
  GRID_GAMES,
  SLIDE_HOLD_MS,
  stepSlider,
  type GridGame,
} from "./gridGames";
import { drawSpeechBubble } from "./speechBubble";
import { t } from "../lib/i18n";

const CELL = 6;
const GAP = 1;
const PITCH = CELL + GAP;
const BORDER_OPACITY = 0.06;
const PEAK_OPACITY = 0.72;
const LOGO_OPACITY = 0.7;
const BUBBLE_OPACITY = 0.9;
const PAC_OPACITY = 0.92;
const GHOST_OPACITY = 0.8;
/** Sprites sit inside their corridor rather than spilling over the walls. */
const SPRITE_SCALE = 0.8;
/** How hard the bubble chases its speaker; sprites move cell by cell. */
const BUBBLE_EASE = 0.2;
const FRAME_MS = 33;

const HEADING: Record<string, { x: number; y: number }> = {
  ArrowUp: { x: 0, y: -1 },
  ArrowDown: { x: 0, y: 1 },
  ArrowLeft: { x: -1, y: 0 },
  ArrowRight: { x: 1, y: 0 },
  w: { x: 0, y: -1 },
  a: { x: -1, y: 0 },
  s: { x: 0, y: 1 },
  d: { x: 1, y: 0 },
};

type Board = {
  game: GridGame;
  arcade: GridArcade;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  bubbleAt: { x: number; y: number } | null;
};

/**
 * Mascot art, traced once over its 8×8 box so each frame is a single fill.
 * Traced on first paint rather than at import, so nothing needs a canvas
 * around just to pull this module in.
 */
const mascotPaths = new Map<string, { rest: Path2D; talk: Path2D }>();

function mascotArt(name: string) {
  const cached = mascotPaths.get(name);
  if (cached) return cached;
  const mascot = PROJECT_MASCOTS.find((it) => it.name === name);
  if (!mascot) return null;
  const art = {
    rest: new Path2D(mascot.restPath),
    talk: new Path2D(mascot.talkPath),
  };
  mascotPaths.set(name, art);
  return art;
}

function parseRgb(value: string, fallback: string) {
  const match = value.match(/\d+/g);
  if (!match || match.length < 3) return fallback;
  return `${match[0]}, ${match[1]}, ${match[2]}`;
}

function parseContentRgb() {
  return parseRgb(getComputedStyle(document.body).color, "235, 238, 241");
}

function parseSurfaceRgb() {
  return parseRgb(
    getComputedStyle(document.body).backgroundColor,
    "20, 22, 25",
  );
}

/** A wedge-mouthed circle facing its heading; `mouth` 1 closes it out entirely. */
function drawPacman(
  ctx: CanvasRenderingContext2D,
  sprite: ArcadeSprite,
  fill: string,
) {
  const px = sprite.cx * PITCH + CELL / 2;
  const py = sprite.cy * PITCH + CELL / 2;
  const radius = (sprite.size * PITCH * SPRITE_SCALE) / 2;
  const facing = Math.atan2(sprite.dy, sprite.dx);
  const mouth = sprite.mouth * Math.PI;

  ctx.fillStyle = fill;
  ctx.beginPath();
  if (mouth <= 0.01) {
    ctx.arc(px, py, radius, 0, Math.PI * 2);
  } else {
    ctx.moveTo(px, py);
    ctx.arc(px, py, radius, facing + mouth, facing - mouth + Math.PI * 2);
    ctx.closePath();
  }
  ctx.fill();
}

/** The mascot's own pixel art, scaled up out of its 8×8 box. */
function drawGhost(
  ctx: CanvasRenderingContext2D,
  sprite: ArcadeSprite,
  fill: string,
) {
  const span = sprite.size * PITCH * SPRITE_SCALE;
  const left = sprite.cx * PITCH + CELL / 2 - span / 2;
  const top = sprite.cy * PITCH + CELL / 2 - span / 2;
  const unit = span / MASCOT_GRID;

  ctx.fillStyle = fill;
  if (sprite.eyes) {
    // Eaten: only the eyes float home, leaning the way they're headed.
    const lean = {
      x: Math.sign(sprite.dx) * unit * 0.6,
      y: Math.sign(sprite.dy) * unit * 0.6,
    };
    for (const offset of [2, 5]) {
      ctx.fillRect(
        left + offset * unit + lean.x,
        top + 3 * unit + lean.y,
        unit,
        unit * 1.5,
      );
    }
    return;
  }

  const art = sprite.mascot ? mascotArt(sprite.mascot) : null;
  if (!art) return;
  ctx.save();
  ctx.translate(left, top);
  ctx.scale(unit, unit);
  ctx.fill(sprite.frame === "talk" ? art.talk : art.rest);
  ctx.restore();
}

export function TerminalGridBackground() {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRefs = useRef<(HTMLCanvasElement | null)[]>([]);
  const boardsRef = useRef<Board[] | null>(null);
  const indexRef = useRef(0);
  const playingRef = useRef(false);
  const scoreRef = useRef(0);
  const livesRef = useRef(0);
  const [playing, setPlaying] = useState(false);
  const [score, setScore] = useState(0);
  const [lives, setLives] = useState(3);
  const [mode, setMode] = useState<ArcadeMode>("mid");
  const [slide, setSlide] = useState<{ index: number; dir: 1 | -1 }>({
    index: 0,
    dir: 1,
  });
  const [hovered, setHovered] = useState(false);

  indexRef.current = slide.index;
  playingRef.current = playing;

  const game = GRID_GAMES[slide.index] ?? GRID_GAMES[0]!;

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;

    const boards: Board[] = [];
    for (let i = 0; i < GRID_GAMES.length; i++) {
      const canvas = canvasRefs.current[i];
      const ctx = canvas?.getContext("2d");
      const spec = GRID_GAMES[i];
      if (!canvas || !ctx || !spec) return;
      boards.push({
        game: spec,
        arcade: spec.create(),
        canvas,
        ctx,
        bubbleAt: null,
      });
    }
    boardsRef.current = boards;

    const logos = Object.fromEntries(
      HARNESSES.map((harness) => {
        const image = new Image();
        image.src = HARNESS_ICONS[harness];
        return [harness, image];
      }),
    ) as Record<HarnessId, HTMLImageElement>;
    const tintCanvas = document.createElement("canvas");
    const tintCtx = tintCanvas.getContext("2d");

    let raf = 0;
    let cols = 0;
    let rows = 0;
    let rgb = parseContentRgb();
    let surfaceRgb = parseSurfaceRgb();
    let lastFrame = 0;

    const layout = () => {
      const { width, height } = root.getBoundingClientRect();
      if (width <= 0 || height <= 0) return;

      const dpr = window.devicePixelRatio || 1;
      const nextCols = Math.ceil(width / PITCH);
      const nextRows = Math.ceil(height / PITCH);

      for (const board of boards) {
        board.canvas.width = Math.floor(width * dpr);
        board.canvas.height = Math.floor(height * dpr);
        board.canvas.style.width = `${width}px`;
        board.canvas.style.height = `${height}px`;
        board.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        board.arcade.resize(nextCols, nextRows);
      }

      cols = nextCols;
      rows = nextRows;
    };

    const paint = (board: Board, width: number, height: number) => {
      const { ctx, arcade } = board;
      const dim = arcade.controlled() ? 1 : board.game.idleDim;
      const stamp = new Float32Array(cols * rows);
      arcade.stamp(stamp, cols, rows);

      const fade = arcade.fade();
      ctx.clearRect(0, 0, width, height);
      ctx.lineWidth = 1;
      ctx.strokeStyle = `rgba(${rgb}, ${BORDER_OPACITY * fade})`;

      const peak = PEAK_OPACITY * dim;
      for (let y = 0; y < rows; y++) {
        for (let x = 0; x < cols; x++) {
          const px = x * PITCH;
          const py = y * PITCH;
          const fillOpacity = (stamp[y * cols + x] ?? 0) * peak;
          if (fillOpacity > 0.02) {
            ctx.fillStyle = `rgba(${rgb}, ${fillOpacity})`;
            ctx.fillRect(px, py, CELL, CELL);
          }
          ctx.strokeRect(px + 0.5, py + 0.5, CELL - 1, CELL - 1);
        }
      }

      const pickup = arcade.logoPickup();
      const logo = pickup ? logos[pickup.harness] : null;
      if (pickup && logo?.complete && logo.naturalWidth) {
        const span = pickup.cells * PITCH - GAP;
        const dx = pickup.x * PITCH;
        const dy = pickup.y * PITCH;
        ctx.globalAlpha = pickup.alpha * LOGO_OPACITY * dim;
        if (MONOCHROME_HARNESSES.has(pickup.harness) && tintCtx) {
          const size = Math.max(1, Math.ceil(span));
          if (tintCanvas.width !== size || tintCanvas.height !== size) {
            tintCanvas.width = size;
            tintCanvas.height = size;
          } else {
            tintCtx.clearRect(0, 0, size, size);
          }
          tintCtx.globalCompositeOperation = "source-over";
          tintCtx.drawImage(logo, 0, 0, size, size);
          tintCtx.globalCompositeOperation = "source-in";
          tintCtx.fillStyle = `rgb(${rgb})`;
          tintCtx.fillRect(0, 0, size, size);
          ctx.drawImage(tintCanvas, dx, dy, span, span);
        } else {
          ctx.drawImage(logo, dx, dy, span, span);
        }
        ctx.globalAlpha = 1;
      }

      for (const sprite of arcade.sprites()) {
        const opacity =
          sprite.alpha *
          dim *
          (sprite.kind === "pacman" ? PAC_OPACITY : GHOST_OPACITY);
        if (opacity <= 0.02) continue;
        const fill = `rgba(${rgb}, ${opacity})`;
        if (sprite.kind === "pacman") drawPacman(ctx, sprite, fill);
        else drawGhost(ctx, sprite, fill);
      }

      const bubble = arcade.speechBubble();
      if (!bubble) {
        board.bubbleAt = null;
        return;
      }

      const target = { x: bubble.x * PITCH, y: bubble.y * PITCH };
      // Ease towards the speaker, but snap when they wrap through a tunnel
      // so the bubble doesn't sail across the whole canvas to catch up.
      const wrapped =
        board.bubbleAt && Math.abs(target.x - board.bubbleAt.x) > width / 3;
      board.bubbleAt =
        !board.bubbleAt || wrapped
          ? target
          : {
              x: board.bubbleAt.x + (target.x - board.bubbleAt.x) * BUBBLE_EASE,
              y: board.bubbleAt.y + (target.y - board.bubbleAt.y) * BUBBLE_EASE,
            };

      drawSpeechBubble(
        ctx,
        bubble.text,
        board.bubbleAt.x,
        board.bubbleAt.y,
        CELL,
        { width, height },
        bubble.alpha * BUBBLE_OPACITY * dim,
        { fg: `rgb(${rgb})`, bg: `rgb(${surfaceRgb})` },
      );
    };

    const draw = (time: number) => {
      if (document.hidden) {
        raf = 0;
        return;
      }
      raf = requestAnimationFrame(draw);
      if (time - lastFrame < FRAME_MS) return;
      const dt = lastFrame ? time - lastFrame : FRAME_MS;
      lastFrame = time;

      const { width, height } = root.getBoundingClientRect();
      if (width <= 0 || height <= 0) return;

      const current = indexRef.current;
      const controlled = playingRef.current;

      for (let i = 0; i < boards.length; i++) {
        const board = boards[i];
        if (!board) continue;
        // A live game only ticks the board you're on; idle keeps neighbours
        // moving so a slide doesn't reveal a frozen frame.
        if (controlled && i !== current) continue;
        board.arcade.step(dt);
        paint(board, width, height);
      }

      const active = boards[current];
      if (!active || !controlled) return;

      const nextScore = active.arcade.score();
      if (nextScore !== scoreRef.current) {
        scoreRef.current = nextScore;
        setScore(nextScore);
      }
      const nextLives = active.arcade.lives();
      if (nextLives !== livesRef.current) {
        livesRef.current = nextLives;
        setLives(nextLives);
      }
    };

    layout();
    raf = requestAnimationFrame(draw);

    const onVisible = () => {
      if (document.hidden || raf) return;
      lastFrame = 0;
      raf = requestAnimationFrame(draw);
    };
    document.addEventListener("visibilitychange", onVisible);

    const resizeObserver = new ResizeObserver(layout);
    resizeObserver.observe(root);

    const themeObserver = new MutationObserver(() => {
      rgb = parseContentRgb();
      surfaceRgb = parseSurfaceRgb();
    });
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["style", "class"],
    });

    return () => {
      cancelAnimationFrame(raf);
      document.removeEventListener("visibilitychange", onVisible);
      resizeObserver.disconnect();
      themeObserver.disconnect();
      boardsRef.current = null;
    };
  }, []);

  const takeControl = useCallback(() => {
    const board = boardsRef.current?.[indexRef.current];
    board?.arcade.setMode(mode);
    board?.arcade.takeControl();
    scoreRef.current = 0;
    livesRef.current = board?.arcade.lives() ?? 0;
    setScore(0);
    setLives(livesRef.current);
    setPlaying(true);
    const active = document.activeElement;
    if (active instanceof HTMLElement) active.blur();
  }, [mode]);

  const pickMode = useCallback((next: ArcadeMode) => {
    const board = boardsRef.current?.[indexRef.current];
    board?.arcade.setMode(next);
    setMode(next);
  }, []);

  const releaseControl = useCallback(() => {
    boardsRef.current?.[indexRef.current]?.arcade.releaseControl();
    scoreRef.current = 0;
    setScore(0);
    setPlaying(false);
  }, []);

  const showGame = useCallback((next: number) => {
    if (next === indexRef.current) return;
    setSlide({
      index: next,
      dir: next > indexRef.current ? 1 : -1,
    });
  }, []);

  useEffect(() => {
    if (playing || hovered || GRID_GAMES.length < 2) return;

    const id = window.setInterval(() => {
      if (document.hidden) return;
      setSlide((current) =>
        stepSlider(current.index, current.dir, GRID_GAMES.length),
      );
    }, SLIDE_HOLD_MS);
    return () => window.clearInterval(id);
  }, [playing, hovered, slide.index]);

  useEffect(() => {
    if (!playing) return;

    const root = rootRef.current;
    root?.focus();
    const onWheel = (event: WheelEvent) => event.preventDefault();
    root?.addEventListener("wheel", onWheel, { passive: false });

    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey || event.isComposing) {
        return;
      }
      const active = document.activeElement;
      if (
        active !== root &&
        !(active instanceof Node && root?.contains(active))
      ) {
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        releaseControl();
        return;
      }
      const heading = HEADING[event.key] ?? HEADING[event.key.toLowerCase()];
      if (!heading) return;
      event.preventDefault();
      event.stopPropagation();
      boardsRef.current?.[indexRef.current]?.arcade.steer(heading.x, heading.y);
    };

    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      root?.removeEventListener("wheel", onWheel);
    };
  }, [playing, releaseControl]);

  return (
    <div
      ref={rootRef}
      tabIndex={playing ? 0 : -1}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      onMouseDown={() => {
        if (playing) rootRef.current?.focus();
      }}
      aria-label={playing ? game.playLabel : undefined}
      className={
        playing
          ? "absolute inset-0 z-20 overflow-hidden bg-background-base outline-none"
          : "group pointer-events-auto absolute inset-x-0 top-0 z-0 h-48"
      }
    >
      <div
        className={
          playing
            ? "absolute inset-0 overflow-hidden"
            : "absolute inset-0 overflow-hidden [mask-image:linear-gradient(to_bottom,#000_65%,transparent_100%)]"
        }
      >
        <div
          className={`flex h-full motion-reduce:transition-none ${
            playing ? "" : "transition-transform duration-700 ease-in-out"
          }`}
          style={{ transform: `translateX(-${slide.index * 100}%)` }}
        >
          {GRID_GAMES.map((item, i) => (
            <div
              key={item.id}
              className="relative h-full w-full min-w-full shrink-0"
            >
              <canvas
                ref={(el) => {
                  canvasRefs.current[i] = el;
                }}
                className="absolute inset-0 h-full w-full"
                aria-hidden
              />
            </div>
          ))}
        </div>
      </div>

      {playing ? (
        <div className="pointer-events-none absolute inset-x-0 top-0 z-10 grid grid-cols-3 items-center px-3 py-2 font-mono text-[11px] tracking-[0.14em] text-content/50">
          <span>
            {t("score {0}", [String(score)])}
            {game.lives ? (
              <span className="ml-3 text-content/35">
                {lives > 0 ? "•".repeat(lives) : t("game over")}
              </span>
            ) : null}
          </span>
          <div className="pointer-events-auto flex justify-center gap-1">
            {ARCADE_MODES.map((id) => {
              const on = mode === id;
              return (
                <button
                  key={id}
                  type="button"
                  tabIndex={-1}
                  aria-pressed={on}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => pickMode(id)}
                  className={`cursor-pointer border px-2 py-1 ${
                    on
                      ? "border-content/40 bg-content/10 text-content"
                      : "border-content/10 text-content/40 hover:border-content/25 hover:text-content/70"
                  }`}
                >
                  {id}
                </button>
              );
            })}
          </div>
          <div className="flex justify-end">
            <button
              type="button"
              tabIndex={-1}
              onClick={releaseControl}
              className="pointer-events-auto cursor-pointer border border-content/20 bg-background-base/70 px-2 py-1 text-content/70 hover:border-content/40 hover:text-content"
            >
              <span className="text-content/35">[</span> {t("release")}{" "}
              <span className="text-content/35">]</span>
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center opacity-0 transition-opacity duration-200 group-hover:opacity-100">
            <button
              type="button"
              tabIndex={-1}
              onMouseDown={(event) => event.preventDefault()}
              onClick={takeControl}
              className="pointer-events-none flex cursor-pointer items-center gap-2 border border-content/25 bg-background-base/80 px-3 py-1.5 font-mono text-[11px] tracking-[0.16em] text-content/85 shadow-lg backdrop-blur-sm group-hover:pointer-events-auto hover:border-content/45 hover:bg-content/10 hover:text-content"
            >
              <span className="text-content/40">[</span>
              {t("take control")}
              <span className="text-content/25">·</span>
              {game.label}
              <span
                className="inline-block h-3 w-1.5 bg-content/75 motion-safe:animate-pulse"
                aria-hidden
              />
              <span className="text-content/40">]</span>
            </button>
          </div>
          {GRID_GAMES.length > 1 ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-1.5 z-10 flex justify-center">
              {GRID_GAMES.map((item, i) => {
                const on = i === slide.index;
                return (
                  <button
                    key={item.id}
                    type="button"
                    tabIndex={-1}
                    aria-label={item.label}
                    aria-pressed={on}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => showGame(i)}
                    className="pointer-events-auto flex h-4 w-4 cursor-pointer items-center justify-center"
                  >
                    <span
                      className={`block h-1.5 w-1.5 ${
                        on
                          ? "bg-content/45"
                          : "bg-content/15 hover:bg-content/30"
                      }`}
                    />
                  </button>
                );
              })}
            </div>
          ) : null}
        </>
      )}
    </div>
  );
}
