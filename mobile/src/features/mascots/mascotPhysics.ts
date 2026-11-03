// A small 2D physics world for mascots tumbling around a screen: circles
// under gravity, bouncing off the screen's edges, each other, and rounded
// rects such as lines of text and buttons. Every function is a worklet, so it
// steps on the UI thread with the frames.

/** A mascot, as a circle. Points and seconds; `angle` in radians. */
export type Body = {
  x: number;
  y: number;
  vx: number;
  vy: number;
  angle: number;
  spin: number;
  radius: number;
  /** Seconds after the start that it is let go, from above the screen. */
  dropAt: number;
  dropped: boolean;
  /** Once wholly on screen, the screen's top edge holds it in too. */
  entered: boolean;
};

/** Something solid on the screen, a rounded rect in screen points. */
export type Collider = {
  x: number;
  y: number;
  width: number;
  height: number;
  radius: number;
};

/** Gravity in g along the screen's axes, x right and y down. */
export type Gravity = { x: number; y: number };

/** One g, in points per second squared: brisk enough to feel weighty. */
const G = 2200;
/** How much speed a bounce keeps. */
const BOUNCE = 0.35;
/** Speed lost along a surface while touching it, per contact. */
const FRICTION = 0.04;
/** How quickly a body in contact takes up the spin of rolling. */
const GRIP = 0.25;
/** How quickly mascots touching each other take up the spin of rubbing past. */
const PAIR_GRIP = 0.1;
/** Radians per second, about two turns: a knock sets them tumbling, never whirling. */
const MAX_SPIN = 12;
const SUBSTEPS = 3;
/**
 * Contacts are settled a few times over each substep. Once, a stack hands
 * each body a share of its neighbour's push, and they keep a speed down they
 * never move at; rolled along a wall, that turned them in place.
 */
const PASSES = 4;

function clamp(value: number, min: number, max: number) {
  "worklet";
  return Math.min(max, Math.max(min, value));
}

/**
 * Settles a body touching a surface whose normal, pointing at the body, is
 * (nx, ny): stops it moving into it, bounces, drags along it, and turns it as
 * it rolls.
 */
function contact(body: Body, nx: number, ny: number) {
  "worklet";
  const along = body.vx * nx + body.vy * ny;
  if (along < 0) {
    body.vx -= (1 + BOUNCE) * along * nx;
    body.vy -= (1 + BOUNCE) * along * ny;
  }
  // Along the surface, clockwise from the normal.
  const tx = -ny;
  const ty = nx;
  const slide = body.vx * tx + body.vy * ty;
  body.vx -= slide * FRICTION * tx;
  body.vy -= slide * FRICTION * ty;
  body.spin += (slide / body.radius - body.spin) * GRIP;
}

function collideWalls(body: Body, width: number, height: number) {
  "worklet";
  const r = body.radius;
  if (body.x < r) {
    body.x = r;
    contact(body, 1, 0);
  } else if (body.x > width - r) {
    body.x = width - r;
    contact(body, -1, 0);
  }
  if (body.y > height - r) {
    body.y = height - r;
    contact(body, 0, -1);
  } else if (body.entered && body.y < r) {
    body.y = r;
    contact(body, 0, 1);
  }
  if (!body.entered && body.y >= r) body.entered = true;
}

function collideBox(body: Body, box: Collider) {
  "worklet";
  const inset = Math.min(box.radius, box.width / 2, box.height / 2);
  // The nearest point on the rect's core; its rounded edge lies `inset` past it.
  const cx = clamp(body.x, box.x + inset, box.x + box.width - inset);
  const cy = clamp(body.y, box.y + inset, box.y + box.height - inset);
  let dx = body.x - cx;
  let dy = body.y - cy;
  let distance = Math.hypot(dx, dy);
  const reach = body.radius + inset;
  if (distance >= reach) return;
  if (distance < 0.0001) {
    // Centre inside the core: out by its nearest side.
    dx = 0;
    dy = -1;
    distance = 0;
  } else {
    dx /= distance;
    dy /= distance;
  }
  const overlap = reach - distance;
  body.x += dx * overlap;
  body.y += dy * overlap;
  contact(body, dx, dy);
}

function collidePair(a: Body, b: Body) {
  "worklet";
  let dx = b.x - a.x;
  let dy = b.y - a.y;
  const distance = Math.hypot(dx, dy);
  const reach = a.radius + b.radius;
  if (distance >= reach || distance < 0.0001) return;
  dx /= distance;
  dy /= distance;
  const half = (reach - distance) / 2;
  a.x -= dx * half;
  a.y -= dy * half;
  b.x += dx * half;
  b.y += dy * half;
  // Equal masses: each takes half the bounce along the line between them.
  const closing = (b.vx - a.vx) * dx + (b.vy - a.vy) * dy;
  if (closing >= 0) return;
  const impulse = (-(1 + BOUNCE) * closing) / 2;
  a.vx -= impulse * dx;
  a.vy -= impulse * dy;
  b.vx += impulse * dx;
  b.vy += impulse * dy;
  // Rubbing past each other turns both toward rolling, the same way round, as
  // the friction between them does. Eased toward rather than added, so a pile
  // resting against itself every step never winds up.
  const glance = (b.vx - a.vx) * -dy + (b.vy - a.vy) * dx;
  a.spin += (glance / a.radius - a.spin) * PAIR_GRIP;
  b.spin += (glance / b.radius - b.spin) * PAIR_GRIP;
}

/** Advances the world by `dt` seconds, `time` seconds after it began. */
export function step(
  bodies: Body[],
  colliders: readonly Collider[],
  gravity: Gravity,
  width: number,
  height: number,
  dt: number,
  time: number,
) {
  "worklet";
  const h = dt / SUBSTEPS;
  for (let s = 0; s < SUBSTEPS; s++) {
    for (const body of bodies) {
      if (!body.dropped) {
        if (time < body.dropAt) continue;
        body.dropped = true;
      }
      // Still above the screen: always pulled in, however the phone is held.
      const gy = body.entered ? gravity.y : Math.max(gravity.y, 0.4);
      body.vx += gravity.x * G * h;
      body.vy += gy * G * h;
      body.vx *= 0.9995;
      body.vy *= 0.9995;
      body.spin *= 0.998;
      body.x += body.vx * h;
      body.y += body.vy * h;
      body.angle += body.spin * h;
    }
    for (let pass = 0; pass < PASSES; pass++) {
      for (let i = 0; i < bodies.length; i++) {
        const a = bodies[i];
        if (!a.dropped) continue;
        for (let j = i + 1; j < bodies.length; j++)
          if (bodies[j].dropped) collidePair(a, bodies[j]);
      }
      for (const body of bodies) {
        if (!body.dropped) continue;
        for (const box of colliders) collideBox(body, box);
        collideWalls(body, width, height);
      }
    }
    // Contacts set spin toward rolling speed, so it is held in after them.
    for (const body of bodies) body.spin = clamp(body.spin, -MAX_SPIN, MAX_SPIN);
  }
}

/**
 * Gravity on the screen from an accelerometer reading in g, as iOS reports
 * it: x toward the right edge, y toward the top. Lying flat, the phone reads
 * next to nothing, so a little pull down is kept and they still settle.
 */
export function screenGravity(x: number, y: number): Gravity {
  "worklet";
  const gx = clamp(x, -3, 3);
  const gy = clamp(-y, -3, 3);
  const strength = Math.hypot(gx, gy);
  return { x: gx, y: gy + Math.max(0, 0.35 - strength) };
}
