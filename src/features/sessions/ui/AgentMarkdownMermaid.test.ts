// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

const engine = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(async () => ({ svg: "<svg></svg>" })),
}));

vi.mock("mermaid", () => ({ default: engine }));

let container: HTMLDivElement;
let root: Root;

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

// Pie slices may be lightened for label contrast; the xy palette keeps the
// accent exactly as set.
function lastAccent(): unknown {
  const config = engine.initialize.mock.calls.at(-1)?.[0] as
    | { themeVariables?: { xyChart?: { plotColorPalette?: string } } }
    | undefined;
  return config?.themeVariables?.xyChart?.plotColorPalette?.split(",")[0];
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.documentElement.style.removeProperty("--user-accent-color");
  document.documentElement.style.removeProperty("--sidebar-opacity");
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

describe("AgentMarkdown Mermaid diagrams", () => {
  it("redraws with the custom accent when appearance changes", async () => {
    document.documentElement.style.setProperty(
      "--user-accent-color",
      "#ff0000",
    );
    act(() =>
      root.render(
        createElement(AgentMarkdown, {
          text: '```mermaid\npie\n  "a" : 1\n```\n',
          cwd: "/repo",
        }),
      ),
    );
    await settle();
    expect(engine.render).toHaveBeenCalledTimes(1);
    expect(lastAccent()).toBe("#ff0000");

    document.documentElement.style.setProperty("--sidebar-opacity", "0.5");
    await settle();
    expect(engine.render).toHaveBeenCalledTimes(1);

    document.documentElement.style.setProperty(
      "--user-accent-color",
      "#00ff00",
    );
    await settle();
    expect(engine.render).toHaveBeenCalledTimes(2);
    expect(lastAccent()).toBe("#00ff00");
  });
});
