import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ComposerAction } from "./Composer";

function renderAction(busy: boolean, hasValue: boolean) {
  return renderToStaticMarkup(
    createElement(ComposerAction, {
      busy,
      hasValue,
      onSend: vi.fn(),
      onStop: vi.fn(),
    }),
  );
}

describe("ComposerAction", () => {
  it("replaces Stop with Send when typing during a running turn", () => {
    const empty = renderAction(true, false);
    expect(empty).toContain('aria-label="Stop"');
    expect(empty).not.toContain('aria-label="Send"');

    const typed = renderAction(true, true);
    expect(typed).toContain('aria-label="Send"');
    expect(typed).not.toContain('aria-label="Stop"');
  });
});
