// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Streamdown } from "streamdown";
import { describe, expect, it } from "vitest";

/**
 * https://github.com/hardbeat920/monocode/issues/218 - a reply with several
 * "\n\n"-separated sections rendered as one dense block.
 *
 * AgentMarkdown passes dir="auto", so Streamdown puts every block in its own
 * `<div dir="..." style="display: contents">`. A display:contents box drops its
 * own margins, so Streamdown's `space-y-4` on the root, which targets exactly
 * those wrappers, paints no gap. Headings, blockquotes and rules were fine
 * because their margin sits on the element itself; paragraphs and lists carry
 * none, so consecutive ones sat flush.
 *
 * The same wrapper is why a `.agent-markdown p:last-child` reset is useless
 * here: every paragraph is the only child of its wrapper, so such a rule
 * matches all of them and cancels the spacing again. This test renders the
 * real Streamdown output and checks the shipped selectors against it, so a
 * rule that matches nothing, or matches every paragraph, fails.
 */

// happy-dom rewrites import.meta.url, so resolve from the vitest root instead.
const CSS_PATH = resolve(process.cwd(), "src/index.css");

const SAMPLE = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";

function renderAgentMarkdown(sample = SAMPLE): string {
  return renderToStaticMarkup(
    createElement(
      Streamdown,
      { dir: "auto", className: "agent-markdown" },
      sample,
    ),
  );
}

type Rule = { selectors: string[]; body: string };

function cssRules(): Rule[] {
  const css = readFileSync(CSS_PATH, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: Rule[] = [];
  for (const match of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    rules.push({
      selectors: match[1].split(",").map((part) => part.trim()),
      body: match[2],
    });
  }
  return rules;
}

type Side = "top" | "bottom";

/** The margin index.css ends up declaring on one side of `el`, or null. */
function declaredMargin(el: Element, side: Side): string | null {
  const longhand = side === "top" ? "margin-top" : "margin-bottom";
  const logical = side === "top" ? "margin-block-start" : "margin-block-end";
  let value: string | null = null;

  for (const rule of cssRules()) {
    const matches = rule.selectors.some((selector) => {
      try {
        return el.matches(selector);
      } catch {
        return false;
      }
    });
    if (!matches) continue;

    for (const declaration of rule.body.split(";")) {
      const [rawProperty, rawValue] = declaration.split(":");
      if (!rawValue) continue;
      const property = rawProperty.trim();
      const parts = rawValue.trim().split(/\s+/);
      if (property === longhand || property === logical) value = parts[0];
      else if (property === "margin-block")
        value = side === "top" ? parts[0] : (parts[1] ?? parts[0]);
      else if (property === "margin")
        value = side === "top" ? parts[0] : (parts[2] ?? parts[0]);
    }
  }
  return value;
}

function isZero(margin: string | null): boolean {
  return margin === null || /^0[a-z%]*$/.test(margin);
}

describe("agent-markdown paragraph spacing", () => {
  it("wraps each block in a display:contents div, so space-y-4 cannot space them", () => {
    document.body.innerHTML = renderAgentMarkdown();
    const root = document.querySelector(".agent-markdown")!;
    const paragraphs = [...root.querySelectorAll("p")];

    expect(root.className).toContain("space-y-4");
    expect(paragraphs).toHaveLength(3);
    for (const paragraph of paragraphs) {
      const wrapper = paragraph.parentElement!;
      expect(wrapper.getAttribute("style")).toContain("display:contents");
      expect(wrapper.parentElement).toBe(root);
    }
  });

  it("leaves a gap between consecutive paragraphs and none after the last", () => {
    document.body.innerHTML = renderAgentMarkdown();
    const paragraphs = [
      ...document.querySelectorAll<HTMLElement>(".agent-markdown p"),
    ];

    paragraphs.forEach((paragraph, index) => {
      const above = declaredMargin(paragraph, "top");
      const below = declaredMargin(paragraph, "bottom");

      if (index > 0) {
        const previousBelow = declaredMargin(paragraphs[index - 1], "bottom");
        expect(above, "expected the full gap between paragraphs").toBe("1rem");
        expect(isZero(previousBelow)).toBe(true);
      } else {
        expect(isZero(above), "no gap above the first paragraph").toBe(true);
      }

      if (index === paragraphs.length - 1) {
        expect(isZero(below), "no gap after the last paragraph").toBe(true);
      }
    });
  });

  it("keeps lists closer to the paragraphs that introduce them", () => {
    document.body.innerHTML = renderAgentMarkdown(
      "Intro.\n\n- First\n- Second\n\nMore context.\n\n1. First\n2. Second",
    );
    const lists = [
      ...document.querySelectorAll<HTMLElement>(
        '.agent-markdown [data-streamdown="unordered-list"], .agent-markdown [data-streamdown="ordered-list"]',
      ),
    ];

    expect(lists).toHaveLength(2);
    for (const list of lists) {
      expect(declaredMargin(list, "top")).toBe("0.5rem");
      expect(isZero(declaredMargin(list, "bottom"))).toBe(true);
    }
  });

  it("does not add a gap before the first visible block", () => {
    document.body.innerHTML = renderAgentMarkdown("\n\nFirst paragraph.");
    const paragraph = document.querySelector<HTMLElement>(".agent-markdown p")!;

    expect(paragraph.parentElement?.previousElementSibling).toBeTruthy();
    expect(isZero(declaredMargin(paragraph, "top"))).toBe(true);
  });
});
