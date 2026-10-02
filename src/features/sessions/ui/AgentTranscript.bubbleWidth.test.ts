// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { compile } from "tailwindcss";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AgentTranscript } from "./AgentTranscript";

/**
 * Chat bubbles are capped at the smaller of their container and 36rem. A bare
 * 36rem cap (`max-w-xl`) looks identical in a wide pane but lets a long prompt
 * overrun a narrow session pane, which is where the bug was reported.
 *
 * happy-dom has no layout engine, so nothing here can read a used width. This
 * renders the transcript, compiles the bubble's real class list with the
 * Tailwind build the app ships, and resolves the `max-width` it declares
 * against a container width — so a regression that drops the container half of
 * the cap resolves to 36rem inside a 22rem pane and fails here.
 */

const REM = 16;
const THEME_ROOT = resolve(process.cwd(), "node_modules/tailwindcss");

let utilities: (candidates: string[]) => string;

const ready = compile(
  '@import "tailwindcss/theme.css" layer(theme);\n@tailwind utilities;',
  {
    loadStylesheet: async (id, base) => ({
      path: id,
      base,
      content: readFileSync(
        resolve(THEME_ROOT, id.replace(/^tailwindcss\//, "")),
        "utf8",
      ),
    }),
  },
).then((compiler) => {
  utilities = compiler.build.bind(compiler);
});

type Rule = { selectors: string[]; body: string };

/** The declarations in one rule body, keyed by property. */
function declarations(body: string): Map<string, string> {
  const parsed = new Map<string, string>();
  for (const declaration of body.split(";")) {
    const separator = declaration.indexOf(":");
    if (separator === -1) continue;
    parsed.set(
      declaration.slice(0, separator).trim(),
      declaration.slice(separator + 1).trim(),
    );
  }
  return parsed;
}

/**
 * Split a compiled stylesheet into rules keyed by class name. Tailwind escapes
 * the punctuation inside an arbitrary value (`min\(100\%\,36rem\)`), so the
 * escapes have to come off before the class name can be compared with
 * `classList`, and only unescaped commas separate a selector list.
 */
function rules(css: string): Rule[] {
  return [
    ...css
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .matchAll(/([^{}]+)\{([^{}]*)\}/g),
  ].map(([, selectorList, body]) => ({
    selectors: selectorList
      .split(/(?<!\\),/)
      .map((selector) =>
        selector
          .trim()
          .replace(/\\(.)/g, "$1")
          .replace(/^\./, ""),
      ),
    body,
  }));
}

function splitArguments(value: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let current = "";
  for (const character of value) {
    if (character === "(") depth += 1;
    if (character === ")") depth -= 1;
    if (character === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
      continue;
    }
    current += character;
  }
  parts.push(current.trim());
  return parts;
}

/** The px value of a CSS length, with percentages taken against `containing`. */
function resolveLength(
  value: string,
  containing: number,
  theme: Map<string, string>,
): number {
  const trimmed = value.trim();
  const variable = trimmed.match(/^var\(--([\w-]+)\)$/);
  if (variable) {
    const resolved = theme.get(variable[1]);
    if (resolved === undefined) throw new Error(`Unresolved ${trimmed}`);
    return resolveLength(resolved, containing, theme);
  }
  const shortest = trimmed.match(/^min\((.+)\)$/);
  if (shortest) {
    return Math.min(
      ...splitArguments(shortest[1]).map((term) =>
        resolveLength(term, containing, theme),
      ),
    );
  }
  if (trimmed.endsWith("%")) {
    return (Number.parseFloat(trimmed) / 100) * containing;
  }
  if (trimmed.endsWith("rem")) return Number.parseFloat(trimmed) * REM;
  if (trimmed.endsWith("px")) return Number.parseFloat(trimmed);
  throw new Error(`Unsupported length: ${trimmed}`);
}

/** The width the last matching utility caps `element` at, or Infinity. */
function maxWidth(element: Element, containing: number): number {
  const compiled = utilities([...element.classList]);
  const theme = new Map<string, string>();
  let declared: string | null = null;
  for (const rule of rules(compiled)) {
    const body = declarations(rule.body);
    // Theme variables are declared on `:root`, whichever utility needs them.
    for (const [property, value] of body) {
      if (property.startsWith("--")) theme.set(property.slice(2), value);
    }
    if (!rule.selectors.some((name) => element.classList.contains(name))) {
      continue;
    }
    const cap = body.get("max-width");
    if (cap) declared = cap;
  }
  return declared === null
    ? Number.POSITIVE_INFINITY
    : resolveLength(declared, containing, theme);
}

let container: HTMLDivElement;
let root: Root;

function renderPrompt() {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          {
            id: "prompt",
            role: "user",
            text: "A long pasted prompt that needs the full bubble width",
          },
        ],
      }),
    ),
  );
}

/**
 * The width the bubble is capped at once its container is `containerWidth`
 * wide, from the `max-width` it declares.
 */
function bubbleCap(containerWidth: number): number {
  const bubble = container.querySelector(".user-message-bubble")!;
  return maxWidth(bubble, containerWidth);
}

beforeEach(async () => {
  await ready;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("stops at the pane in a narrow session pane", () => {
  renderPrompt();
  expect(bubbleCap(22 * REM)).toBe(22 * REM);
});

it("stops at 36rem in a pane wider than that", () => {
  renderPrompt();
  expect(bubbleCap(80 * REM)).toBe(36 * REM);
});

it("keeps the 36rem cap when the pane is exactly that wide", () => {
  renderPrompt();
  expect(bubbleCap(36 * REM)).toBe(36 * REM);
});
