import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { UsageFooter } from "./UsageFooter";

describe("UsageFooter terminal control", () => {
  it("replaces the generic terminal button with the live process control", () => {
    const markup = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        terminals: [
          {
            id: "terminal-1",
            process: "npm",
            cwd: "/repo",
            label: "repo",
          },
        ],
        terminalOpen: true,
        onToggleTerminal: vi.fn(),
        onToggleProjectTerminal: vi.fn(),
        projectTerminalOpen: true,
      }),
    );

    expect(markup).toContain(">npm</span>");
    expect(markup).not.toContain(">Terminal</span>");
    expect(markup.match(/<button/g)).toHaveLength(1);
  });

  it("keeps the generic terminal button when no process is running", () => {
    const markup = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        onToggleProjectTerminal: vi.fn(),
      }),
    );

    expect(markup).toContain(">Terminal</span>");
    expect(markup.match(/<button/g)).toHaveLength(1);
  });

  it("shows the terminal button pressed only while the dock is open", () => {
    const render = (projectTerminalOpen: boolean) =>
      renderToStaticMarkup(
        createElement(UsageFooter, {
          providers: [],
          onToggleProjectTerminal: vi.fn(),
          projectTerminalOpen,
        }),
      );

    expect(render(false)).toContain('aria-pressed="false"');
    expect(render(false)).toContain("Show Terminal");
    expect(render(true)).toContain('aria-pressed="true"');
    expect(render(true)).toContain("Hide Terminal");
  });
});
