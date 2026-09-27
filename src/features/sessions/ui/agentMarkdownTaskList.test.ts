// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

/**
 * GFM task lists reach the DOM as native, unstyled `<input type="checkbox">`
 * boxes sitting beside a `list-disc` bullet, so every checkbox rendered as a
 * chunky default widget with a dot next to it.
 *
 * The restyling is pure CSS, which means a selector that stops matching fails
 * *silently*: no build error, no failing test, the preview just quietly goes
 * back to looking wrong. So these tests render the real Streamdown output and
 * run the shipped selectors against it, which catches dead CSS.
 *
 * Two list shapes have to keep working. Streamdown renders `MarkdownLi` with
 * `[&>p]:inline`, and a loose list wraps item text in `<p>` while a tight list
 * leaves it as bare text inside the `li` - so the item's own checkbox is a
 * direct child in one and a grandchild in the other. Real documents use both.
 *
 * Nested lists matter for the dimming rule: a plain `li:has(input:checked)`
 * would also match an ancestor that merely *contains* a checked item, fading
 * text the user never ticked.
 */

// happy-dom rewrites import.meta.url, so resolve from the vitest root instead.
const CSS_PATH = resolve(process.cwd(), "src/styles/index.css");

const TIGHT = "- [ ] open\n- [x] done\n";
const LOOSE = "- [ ] open para\n\n- [x] done para\n";
const NESTED = "- outer item\n  - [x] inner checked\n";

function renderInto(sample: string): void {
  document.body.innerHTML = renderToStaticMarkup(
    createElement(AgentMarkdown, { text: sample }),
  );
}

type CssRule = { selector: string; body: string };

/** Every rule in the shipped stylesheet that targets a markdown checkbox. */
function checkboxRules(): CssRule[] {
  const css = readFileSync(CSS_PATH, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .map((match) => ({ selector: (match[1] ?? "").trim(), body: match[2] ?? "" }))
    .filter((rule) => rule.selector.includes('input[type="checkbox"]'));
}

/** How many elements in the current document a shipped selector actually hits. */
function hitCount(selector: string): number {
  return document.querySelectorAll(selector).length;
}

function rule(predicate: (rule: CssRule) => boolean): string {
  const found = checkboxRules().find(predicate);
  expect(found, "no shipped checkbox rule matched the predicate").toBeDefined();
  return found!.selector;
}

describe("markdown task list checkboxes", () => {
  it("ships no dead checkbox selectors", () => {
    renderInto([TIGHT, LOOSE, NESTED].join("\n\n"));
    const selectors = checkboxRules().map((cssRule) => cssRule.selector);

    expect(selectors.length).toBeGreaterThan(0);
    for (const selector of selectors) {
      expect(
        hitCount(selector),
        `selector matches nothing in real output: ${selector}`,
      ).toBeGreaterThan(0);
    }
  });

  it("drops the bullet on task items but keeps it on plain list items", () => {
    const bullet = rule((cssRule) => cssRule.body.includes("list-style: none"));

    renderInto(`${TIGHT}\n- plain item\n`);

    for (const item of document.querySelectorAll("li.task-list-item")) {
      expect(item.matches(bullet)).toBe(true);
    }
    for (const item of document.querySelectorAll("li:not(.task-list-item)")) {
      expect(item.matches(bullet)).toBe(false);
    }
  });

  it.each([
    ["tight", TIGHT],
    ["loose", LOOSE],
  ])("styles the checkbox in a %s list", (_shape, sample) => {
    const box = rule((cssRule) => cssRule.body.includes("appearance: none"));

    renderInto(sample);
    const boxes = document.querySelectorAll("li input[type=checkbox]");

    expect(boxes.length).toBe(2);
    for (const element of boxes) {
      expect(element.matches(box)).toBe(true);
    }
  });

  it("gates the checkmark on :checked", () => {
    const checked = rule((cssRule) => cssRule.selector.includes(":checked{") === false && cssRule.selector.includes(":checked"));

    renderInto(TIGHT);

    expect(hitCount(checked)).toBe(1);
    const hit = document.querySelector(checked)!;
    expect(hit.matches("input[checked]")).toBe(true);
  });

  it("draws the tick with the accent instead of a native glyph", () => {
    const checked = checkboxRules().find((cssRule) =>
      cssRule.selector.includes(":checked"),
    )!.body;

    expect(checked).toContain("background-color: var(--color-accent)");
    expect(checked).toContain("background-image:");
    expect(checked).toContain("data:image/svg+xml");
    expect(checked).toMatch(/<svg|<path|svg/);
  });

  it("suppresses the native widget and keeps the text axis", () => {
    const box = rule((cssRule) => cssRule.body.includes("appearance: none"));
    const body = checkboxRules().find((r) => r.selector === box)!.body;

    expect(body).toContain("-webkit-appearance: none");
    expect(body).toContain("box-sizing: border-box");
  });

  it("pulls the box into the bullet gutter using the shared list indent", () => {
    // The box has to reclaim the gutter the suppressed bullet left behind, and
    // it has to do so via the same custom property the lists pad with -
    // otherwise editing one literal silently misaligns every task list.
    const css = readFileSync(CSS_PATH, "utf8");
    const box = checkboxRules().find((cssRule) =>
      cssRule.body.includes("appearance: none"),
    )!.body;
    expect(box).toContain(
      "margin-inline-start: calc(-1 * var(--agent-markdown-list-indent))",
    );

    const indent = css.match(
      /\.agent-markdown \{([\s\S]*?)--agent-markdown-list-indent:\s*([\d.]+rem)/,
    );
    expect(indent, "list indent custom property is never declared").not.toBeNull();

    // Every list that establishes a gutter must indent by that same property.
    const listRules = [
      ...css.matchAll(
        /([^{}]*data-streamdown="(?:ordered|unordered)-list"\][^{}]*)\{([\s\S]*?)\}/g,
      ),
    ].filter((match) => match[2]?.includes("padding-inline-start"));
    expect(listRules.length).toBeGreaterThan(0);
    for (const match of listRules) {
      expect(match[2]).toContain(
        "padding-inline-start: var(--agent-markdown-list-indent)",
      );
    }
  });

  it("repaints the box in system colors under forced colors", () => {
    const css = readFileSync(CSS_PATH, "utf8");
    const block = css.match(
      /@media \(forced-colors: active\) \{([\s\S]*?)\n\}\n/,
    );
    expect(block, "no forced-colors block for markdown checkboxes").not.toBeNull();

    const body = block![1]!;
    // The themed border and fill vanish against the system Canvas, so both
    // states have to be restated in system colors.
    expect(body).toContain("border-color: CanvasText");
    expect(body).toContain("background-color: Canvas");
    expect(body).toContain("background-color: Highlight");
    expect(body).toContain(":checked");
  });

  it("dims a checked item's own text in both list shapes", () => {
    const dim = checkboxRules()
      .filter((cssRule) => cssRule.selector.includes(":checked"))
      .map((cssRule) => cssRule.selector);

    // Tight: the text is a bare text node, so the rule targets the li itself.
    renderInto(TIGHT);
    const tightDone = document.querySelector<HTMLElement>(
      "li.task-list-item:has(> input[checked])",
    )!;
    expect(dim.some((selector) => tightDone.matches(selector))).toBe(true);

    // Loose: the text lives in a <p>, so the rule has to reach the paragraph.
    renderInto(LOOSE);
    const looseDone = document.querySelector<HTMLElement>(
      "li.task-list-item:has(> p > input[checked]) > p",
    )!;
    expect(dim.some((selector) => looseDone.matches(selector))).toBe(true);
  });

  it("leaves unticked items at full ink", () => {
    const dim = checkboxRules()
      .filter((cssRule) => cssRule.selector.includes(":checked"))
      .map((cssRule) => cssRule.selector);

    for (const sample of [TIGHT, LOOSE]) {
      renderInto(sample);
      const open = document.querySelector<HTMLElement>(
        "li.task-list-item:has(input:not([checked]))",
      )!;
      expect(dim.some((selector) => open.matches(selector))).toBe(false);
    }
  });

  it("does not dim an ancestor that only contains a checked item", () => {
    const dim = checkboxRules()
      .filter((cssRule) => cssRule.selector.includes(":checked"))
      .map((cssRule) => cssRule.selector);

    renderInto(NESTED);

    const parent = document.querySelector<HTMLElement>(
      "li:not(.task-list-item)",
    )!;
    expect(parent.textContent).toContain("outer item");
    expect(dim.some((selector) => parent.matches(selector))).toBe(false);
  });
});
