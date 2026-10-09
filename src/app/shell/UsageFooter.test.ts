import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { UsageFooter } from "./UsageFooter";
import {
  clearCachedRateLimits,
  setCachedRateLimits,
} from "../../features/providers/model/rateLimitsCache";
import { unavailableRateLimits } from "../../features/providers/model/rateLimits";

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
  it("shows the remote usage limitation instead of the desktop snapshot", () => {
    clearCachedRateLimits();
    setCachedRateLimits(
      "droid",
      "default",
      unavailableRateLimits("droid", "Desktop account"),
    );
    setCachedRateLimits(
      "droid",
      "default",
      unavailableRateLimits(
        "droid",
        "Usage is unavailable for remote sessions",
      ),
      "host-b",
    );
    const markup = renderToStaticMarkup(
      createElement(UsageFooter, {
        providers: ["droid"],
        session: { harness: "droid", environmentId: "host-b" },
      }),
    );
    expect(markup).toContain("Usage is unavailable for remote sessions");
    expect(markup).not.toContain("Desktop account");
    clearCachedRateLimits();
  });
});
