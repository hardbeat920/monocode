// MonoCode agent cursor: a visible pointer that shows the user where the
// agent moves, clicks and types in a tab it controls.
//
// Called with Runtime.callFunctionOn against the "__monocode" isolated
// world's execution context of the tab's main frame (the same world as
// marker.js), so the page's own scripts never see these functions or the
// state kept on the world's global object.
//
// What the page can see is one element appended to <html>: a
// <monocode-cursor> host with a closed shadow root. It is aria-hidden (never
// in the accessibility snapshot), has pointer-events: none (never the target
// of a click or a hit test), holds no text, and is inserted before an
// action's observation starts. Moves and clicks only change nodes inside the
// closed root, so the DOM mutation count of the post-action observation
// stays the page's own. Styles are set through the CSSOM and motion through
// the Web Animations API, which a page's CSP does not block.
//
// op:
//   "move"    (x, y, fromX, fromY, jump) -> { ms }: glide the tip to (x, y),
//             tab viewport coordinates; `ms` is how long the glide takes
//             (0 when it jumped: hidden tab, reduced motion, or `jump`).
//             fromX/fromY is where the cursor was in the previous document,
//             used when this document has no cursor yet.
//   "click"   -> { ok }: press animation and a ring at the tip.
//   "conceal" -> { ok }: hide while a screenshot is taken.
//   "reveal"  -> { ok }: show again after the screenshot.
//   "remove"  -> { ok }: take the cursor off the page (tab released).
function monocodeCursor(op, x, y, fromX, fromY, jump) {
  const KEY = "__monocodeCursor";
  const INK = "#1f1f1f";
  const ACCENT = "hsl(211 92% 62%)";
  const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";
  const IDLE_MS = 8000;
  let cursor = globalThis[KEY];

  if (op === "remove") {
    if (cursor) {
      cursor.remove();
    }
    return { ok: true };
  }
  if (!cursor && op !== "move") {
    return { ok: false };
  }

  // The host is in the page's document, so its own styles are !important
  // against page CSS. Everything inside the closed root is out of reach of
  // page CSS and stays animatable (an !important value beats an animation).
  const css = (el, props, important) => {
    for (const name of Object.keys(props)) {
      el.style.setProperty(name, String(props[name]), important ? "important" : "");
    }
    return el;
  };
  const motion = () => {
    if (document.visibilityState !== "visible") {
      return false;
    }
    try {
      return !matchMedia("(prefers-reduced-motion: reduce)").matches;
    } catch (_) {
      return true;
    }
  };

  if (!cursor) {
    const host = css(document.createElement("monocode-cursor"), {
      all: "initial",
      display: "block",
      position: "fixed",
      top: "0",
      left: "0",
      right: "auto",
      bottom: "auto",
      width: "0",
      height: "0",
      margin: "0",
      padding: "0",
      border: "0",
      background: "transparent",
      overflow: "visible",
      "pointer-events": "none",
      "user-select": "none",
      "z-index": "2147483647",
      visibility: "visible",
    }, true);
    host.setAttribute("aria-hidden", "true");
    const root = host.attachShadow({ mode: "closed" });

    const wrap = css(document.createElement("div"), {
      position: "absolute",
      left: "0",
      top: "0",
      "will-change": "transform",
      "pointer-events": "none",
    });
    const ring = css(document.createElement("div"), {
      position: "absolute",
      left: "-14px",
      top: "-14px",
      width: "28px",
      height: "28px",
      "box-sizing": "border-box",
      "border-radius": "50%",
      border: "2px solid " + ACCENT,
      background: "color-mix(in srgb, " + ACCENT + " 18%, transparent)",
      opacity: "0",
    });
    const SVG = "http://www.w3.org/2000/svg";
    const arrow = document.createElementNS(SVG, "svg");
    arrow.setAttribute("width", "22");
    arrow.setAttribute("height", "24");
    arrow.setAttribute("viewBox", "0 0 22 24");
    css(arrow, {
      position: "absolute",
      // The tip of the path sits at (3, 2) in the SVG.
      left: "-3px",
      top: "-2px",
      overflow: "visible",
      "transform-origin": "3px 2px",
      filter: "drop-shadow(0 1px 2px rgba(0, 0, 0, 0.35))",
    });
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", "M3 2 L3 19.5 L7.6 15.2 L10.6 21.8 L13.8 20.4 L10.9 13.9 L17 13.9 Z");
    path.setAttribute("fill", INK);
    path.setAttribute("stroke", "#ffffff");
    path.setAttribute("stroke-width", "1.6");
    path.setAttribute("stroke-linejoin", "round");
    arrow.appendChild(path);
    const label = css(document.createElement("div"), {
      position: "absolute",
      left: "15px",
      top: "21px",
      display: "flex",
      "align-items": "center",
      gap: "5px",
      padding: "3px 8px 3px 7px",
      "border-radius": "999px",
      background: INK,
      color: "#ffffff",
      border: "1px solid rgba(255, 255, 255, 0.2)",
      "box-shadow": "0 2px 8px rgba(0, 0, 0, 0.3)",
      font: '600 11px/14px -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
      "letter-spacing": "0.01em",
      "white-space": "nowrap",
    });
    const diamond = css(document.createElement("span"), {
      display: "inline-block",
      width: "6px",
      height: "6px",
      background: ACCENT,
      transform: "rotate(45deg)",
    });
    label.appendChild(diamond);
    label.appendChild(document.createTextNode("MonoCode"));
    root.appendChild(wrap);
    wrap.appendChild(ring);
    wrap.appendChild(arrow);
    wrap.appendChild(label);

    let idle = 0;
    let fade = null;
    cursor = {
      host,
      wrap,
      ring,
      arrow,
      pos: null,
      glide: null,
      // Put the host back if the page replaced what was under <html>.
      attach() {
        if (!host.isConnected && document.documentElement) {
          document.documentElement.appendChild(host);
        }
      },
      // Fully visible now, faded out after a while without actions.
      wake() {
        clearTimeout(idle);
        if (fade) {
          fade.cancel();
          fade = null;
        }
        css(wrap, { opacity: "1" });
        idle = setTimeout(() => {
          fade = wrap.animate([{ opacity: 1 }, { opacity: 0 }], {
            duration: 600,
            easing: "ease-out",
          });
          fade.onfinish = () => css(wrap, { opacity: "0" });
        }, IDLE_MS);
      },
      remove() {
        clearTimeout(idle);
        host.remove();
        if (globalThis[KEY] === cursor) {
          delete globalThis[KEY];
        }
      },
    };
    globalThis[KEY] = cursor;
    // Taking the tab back (marker release) takes the cursor off too.
    globalThis.__monocodeCursorRemove = () => {
      const current = globalThis[KEY];
      if (current) {
        current.remove();
      }
      return true;
    };
  }

  const at = (p) => "translate3d(" + p.x + "px, " + p.y + "px, 0)";

  if (op === "move") {
    const to = { x: Number(x) || 0, y: Number(y) || 0 };
    const first = !cursor.pos;
    let from = cursor.pos;
    if (!from && Number.isFinite(fromX) && Number.isFinite(fromY)) {
      from = { x: fromX, y: fromY };
    }
    if (!from) {
      // First appearance: come in from a little below and to the left.
      from = { x: Math.max(0, to.x - 90), y: to.y + 70 };
    }
    cursor.attach();
    cursor.wake();
    if (cursor.glide) {
      cursor.glide.cancel();
      cursor.glide = null;
    }
    cursor.pos = to;
    css(cursor.wrap, { transform: at(to) });
    if (jump || !motion()) {
      return { ms: 0 };
    }
    const distance = Math.hypot(to.x - from.x, to.y - from.y);
    if (distance < 2) {
      return { ms: 0 };
    }
    const ms = Math.round(Math.min(200, Math.max(90, 70 + distance * 0.2)));
    cursor.glide = cursor.wrap.animate([{ transform: at(from) }, { transform: at(to) }], {
      duration: ms,
      easing: EASE,
    });
    if (first) {
      cursor.wrap.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: Math.min(ms, 200),
        easing: "ease-out",
      });
    }
    return { ms };
  }

  if (op === "click") {
    cursor.wake();
    if (motion()) {
      cursor.arrow.animate(
        [{ transform: "scale(1)" }, { transform: "scale(0.82)" }, { transform: "scale(1)" }],
        { duration: 240, easing: "ease-out" }
      );
      cursor.ring.animate(
        [
          { transform: "scale(0.35)", opacity: 0.95 },
          { transform: "scale(1.7)", opacity: 0 },
        ],
        { duration: 520, easing: EASE }
      );
    }
    return { ok: true };
  }

  if (op === "conceal") {
    css(cursor.host, { visibility: "hidden" }, true);
    return { ok: true };
  }

  if (op === "reveal") {
    css(cursor.host, { visibility: "visible" }, true);
    return { ok: true };
  }

  return { ok: false };
}
