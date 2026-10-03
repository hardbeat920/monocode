import type { Plugin } from "vite";

// macOS 12's system WebView is WebKit ~16.0, which predates color-mix()
// (Safari 16.2). Tailwind's fallback for var-based colors is the opaque color,
// so `bg-content/10` renders as a solid near-white block there. Mixing a color
// with transparent is the same as giving it alpha, so rewrite those mixes to
// `fn(args / pct)` and let the rewritten rules apply unconditionally.

const THEMED_HSL =
  /^hsl\(var\(--theme-hue\) var\(--theme-saturation\) (var\(--[\w-]+\))\)$/;

function splitTopLevel(input: string, separator: RegExp): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    else if (depth === 0 && separator.test(ch)) {
      parts.push(input.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(input.slice(start));
  return parts.map((part) => part.trim()).filter((part) => part.length > 0);
}

export function rewriteColorMix(css: string): string {
  const definitions = new Map<string, string>();
  const definition = /--([\w-]+):\s*([^;{}]+)[;}]/g;
  for (let match; (match = definition.exec(css));) {
    if (!definitions.has(match[1])) {
      definitions.set(match[1], match[2].trim());
    }
  }

  const resolve = (expr: string, depth = 0): string => {
    const reference = /^var\(--([\w-]+)\)$/.exec(expr);
    if (!reference || depth > 8) return expr;
    const value = definitions.get(reference[1]);
    return value ? resolve(value, depth + 1) : expr;
  };

  const withAlpha = (color: string, amount: string): string | null => {
    const value = resolve(color);
    const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value);
    if (hex) {
      const digits =
        hex[1].length === 3
          ? hex[1].replace(/./g, (digit) => digit + digit)
          : hex[1];
      const [r, g, b] = [0, 2, 4].map((i) =>
        parseInt(digits.slice(i, i + 2), 16),
      );
      return `rgb(${r} ${g} ${b} / ${amount})`;
    }
    const fn = /^(hsl|rgb|oklch|oklab|lab|lch)\((.*)\)$/i.exec(value);
    if (fn && !fn[2].includes("/") && !fn[2].includes(",")) {
      return `${fn[1]}(${fn[2].trim()} / ${amount})`;
    }
    return null;
  };

  // Theme colors share hue and saturation and differ only in lightness, so an
  // opaque mix of two of them is a mix of their lightness values.
  const mixThemed = (
    color: string,
    amount: string,
    other: string,
  ): string | null => {
    const a = THEMED_HSL.exec(resolve(color));
    const b = THEMED_HSL.exec(resolve(other));
    const percent = /^(\d+(?:\.\d+)?)%$/.exec(amount);
    if (!a || !b || !percent) return null;
    const p = Number(percent[1]) / 100;
    return `hsl(var(--theme-hue) var(--theme-saturation) calc(${a[1]} * ${p} + ${b[1]} * ${1 - p}))`;
  };

  let out = "";
  let index = 0;
  while (index < css.length) {
    const start = css.indexOf("color-mix(", index);
    if (start === -1) {
      out += css.slice(index);
      break;
    }
    let depth = 0;
    let end = start + "color-mix".length;
    for (; end < css.length; end++) {
      if (css[end] === "(") depth++;
      else if (css[end] === ")" && --depth === 0) break;
    }
    const inner = rewriteColorMix(css.slice(start + "color-mix(".length, end));
    const args = splitTopLevel(inner, /,/);
    let replacement: string | null = null;
    if (args.length === 3) {
      const first = splitTopLevel(args[1], /\s/);
      if (first.length === 2) {
        replacement =
          args[2] === "transparent"
            ? withAlpha(first[0], first[1])
            : mixThemed(first[0], first[1], args[2]);
      }
    }
    out += css.slice(index, start) + (replacement ?? `color-mix(${inner})`);
    index = end + 1;
  }
  return out;
}

export function legacyColorMix(): Plugin {
  return {
    name: "legacy-color-mix",
    apply: "build",
    enforce: "post",
    generateBundle(_, bundle) {
      for (const asset of Object.values(bundle)) {
        if (asset.type !== "asset" || !asset.fileName.endsWith(".css")) {
          continue;
        }
        asset.source = rewriteColorMix(String(asset.source))
          .split("@supports (color:color-mix(in lab,red,red))")
          .join("@supports (color:red)");
      }
    },
  };
}
