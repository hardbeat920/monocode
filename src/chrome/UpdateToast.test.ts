import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { UpdateToastCard } from "./UpdateToast";

describe("UpdateToastCard", () => {
  it("announces the update and names both actions", () => {
    const markup = renderToStaticMarkup(
      createElement(UpdateToastCard, {
        update: { version: "0.1.23" },
        onOpen: vi.fn(),
        onDismiss: vi.fn(),
      }),
    );

    expect(markup).toContain('role="status"');
    expect(markup).toContain("MonoCode updated to 0.1.23");
    expect(markup).toContain("What&#x27;s new");
    expect(markup).toContain('aria-label="Dismiss update notification"');
  });
});
