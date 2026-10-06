// @vitest-environment happy-dom
import { act, createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageFooter } from "./UsageFooter";

let container: HTMLDivElement;
let root: Root | null = null;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
});

afterEach(() => {
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  container.remove();
  vi.unstubAllGlobals();
});

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
        onNewTerminal: vi.fn(),
        onShowTerminal: vi.fn(),
        projectTerminalActive: true,
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
        onNewTerminal: vi.fn(),
      }),
    );

    expect(markup).toContain(">Terminal</span>");
    expect(markup.match(/<button/g)).toHaveLength(1);
  });

  it("marks the terminal button pressed only while the dock is open", () => {
    const open = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        onNewTerminal: vi.fn(),
        onToggleProjectTerminal: vi.fn(),
        projectTerminalActive: true,
        projectTerminalOpen: true,
      }),
    );
    const collapsed = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: [],
        onNewTerminal: vi.fn(),
        onToggleProjectTerminal: vi.fn(),
        projectTerminalActive: true,
        projectTerminalOpen: false,
      }),
    );

    // Label still reflects "has terminals" in both states.
    expect(open).toContain('aria-label="Terminal"');
    expect(collapsed).toContain('aria-label="Terminal"');
    expect(open).toContain('aria-pressed="true"');
    expect(collapsed).toContain('aria-pressed="false"');
  });

  it("toggles the dock instead of only showing it", () => {
    const onToggleProjectTerminal = vi.fn();
    const onShowTerminal = vi.fn();
    root = createRoot(container);
    act(() =>
      root!.render(
        createElement(UsageFooter, {
          providers: [],
          onNewTerminal: vi.fn(),
          onShowTerminal,
          onToggleProjectTerminal,
          projectTerminalActive: true,
          projectTerminalOpen: true,
        }),
      ),
    );

    const button = container.querySelector('button[aria-label="Terminal"]');
    expect(button).not.toBeNull();
    act(() => (button as HTMLButtonElement).click());
    expect(onToggleProjectTerminal).toHaveBeenCalledTimes(1);
    expect(onShowTerminal).not.toHaveBeenCalled();
  });
});
