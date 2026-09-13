import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ModalPanel } from "./Modal";

describe("ModalPanel", () => {
  it("names the dialog and close action", () => {
    const markup = renderToStaticMarkup(
      createElement(ModalPanel, {
        title: "Example",
        description: "A reusable shell",
        closeDisabled: true,
        onClose: vi.fn(),
        children: "Body",
      }),
    );

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain("modal-panel");
    expect(markup).toContain("Example");
    expect(markup).toContain("A reusable shell");
    expect(markup).toContain('title="A reusable shell"');
    expect(markup).toContain("Body");
    expect(markup).toContain('aria-label="Close"');
    expect(markup).toContain("disabled");
  });
});
