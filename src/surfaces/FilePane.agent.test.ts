// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newAgentTab, newEditorPane } from "../lib/layout";
import { newSession } from "../lib/session";
import { FilePane } from "./FilePane";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => []),
  isTauri: () => false,
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

describe("file pane agent tabs", () => {
  let root: Root;
  let container: HTMLDivElement;
  let props: ComponentProps<typeof FilePane>;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    const file = newAgentTab("Audit the engine", "/repo", {
      sessionId: "worker",
      leadId: "lead",
      harness: "codex",
    });
    const noop = () => {};
    props = {
      pane: newEditorPane(file),
      focused: true,
      dirtyFileIds: new Set(),
      fileErrorCounts: new Map(),
      sessions: [],
      onFocus: noop,
      onSelectFile: noop,
      onCloseFile: noop,
      onCloseOtherFiles: noop,
      onDirtyChange: noop,
      onErrorCountChange: noop,
      onReorderFiles: noop,
      onOpenFile: noop,
      onUpdatePlan: noop,
      onBuildPlan: noop,
    };
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("shows a worker transcript after its session arrives", async () => {
    await act(async () => root.render(createElement(FilePane, props)));
    expect(container.textContent).toContain("This agent is no longer running");

    const session = {
      ...newSession("codex", "/repo"),
      id: "worker",
      blocks: [
        { id: "u1", role: "user" as const, text: "Audit the engine" },
        { id: "a1", role: "assistant" as const, text: "Looking now" },
      ],
    };
    await act(async () =>
      root.render(createElement(FilePane, { ...props, sessions: [session] })),
    );
    expect(container.textContent).toContain("Looking now");
    expect(container.textContent).not.toContain(
      "This agent is no longer running",
    );

    await act(async () =>
      root.render(
        createElement(FilePane, {
          ...props,
          sessions: [
            {
              ...session,
              blocks: [
                ...session.blocks,
                { id: "a2", role: "assistant" as const, text: "Done" },
              ],
            },
          ],
        }),
      ),
    );
    expect(container.textContent).toContain("Done");
  });
});
