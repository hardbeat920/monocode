// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { newSession } from "../model/session";
import { AgentTabView } from "./AgentTabView";
import { AgentTranscript } from "./AgentTranscript";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (_command, args) => args?.note),
}));

it("waits for note storage, reports failure, and lets the user retry", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  let rejectSave!: (error: Error) => void;
  vi.mocked(invoke).mockImplementationOnce(
    () =>
      new Promise((_resolve, reject) => {
        rejectSave = reject;
      }),
  );
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    act(() =>
      root.render(
        createElement(AgentTabView, {
          title: "Worker",
          visible: true,
          session: {
            ...newSession("codex", "/project"),
            blocks: [{ id: "p", role: "user", text: "Keep me" }],
          },
        }),
      ),
    );
    const button = container.querySelector<HTMLButtonElement>(
      '[aria-label="Save as note"]',
    )!;
    await act(async () => button.click());
    expect(container.querySelector('[aria-label="Saved to Notes"]')).toBeNull();
    expect(button.disabled).toBe(true);
    await act(async () => rejectSave(new Error("Storage unavailable")));
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "Storage unavailable",
    );
    expect(button.disabled).toBe(false);
    await act(async () => button.click());
    expect(button.getAttribute("aria-label")).toBe("Saved to Notes");
    expect(container.querySelector('[role="alert"]')).toBeNull();
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});

it.each(["worker", "main"])(
  "offers the correct selection actions in %s chat",
  async (view) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    localStorage.clear();
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const session = {
      ...newSession("codex", "/project"),
      blocks: [
        { id: "prompt", role: "user" as const, text: "Keep this sentence" },
      ],
    };
    const save = vi.fn();
    const chat = vi.fn();
    try {
      act(() =>
        root.render(
          view === "worker"
            ? createElement(AgentTabView, {
                title: "Worker",
                session,
                visible: true,
              })
            : createElement(AgentTranscript, {
                blocks: session.blocks,
                onSaveSelectionNote: save,
                onAddToChat: chat,
              }),
        ),
      );
      const text = container.querySelector("pre")!.firstChild!;
      const range = document.createRange();
      range.selectNodeContents(text);
      vi.spyOn(range, "getClientRects").mockReturnValue({
        length: 0,
      } as DOMRectList);
      vi.spyOn(range, "getBoundingClientRect").mockReturnValue(
        new DOMRect(10, 20, 120, 20),
      );
      window.getSelection()!.removeAllRanges();
      window.getSelection()!.addRange(range);
      await act(async () => {
        document.dispatchEvent(new Event("selectionchange"));
        await new Promise((resolve) => window.requestAnimationFrame(resolve));
      });
      const toolbar = document.querySelector(
        '[aria-label="Selected text actions"]',
      );
      expect(toolbar).not.toBeNull();
      expect(toolbar!.textContent).toContain("Add to notes");
      expect(toolbar!.textContent!.includes("Add to chat")).toBe(
        view === "main",
      );
      const notes = [...toolbar!.querySelectorAll("button")].find(
        (button) => button.textContent === "Add to notes",
      )!;
      await act(async () => notes.click());
      if (view === "main")
        expect(save).toHaveBeenCalledWith("Keep this sentence");
      else
        expect(invoke).toHaveBeenCalledWith("notes_upsert", {
          note: expect.objectContaining({
            body: "Keep this sentence",
            sourceSessionId: session.id,
          }),
        });
    } finally {
      act(() => root.unmount());
      window.getSelection()?.removeAllRanges();
      container.remove();
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  },
);

it("saves worker prompts and responses with their source session and project", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const session = {
    ...newSession("codex", "/project"),
    title: "Inspect notifications",
    busy: false,
    blocks: [
      {
        id: "prompt",
        role: "user" as const,
        internal: true,
        durationMs: 500,
        text: "Check notifications",
      },
      { id: "answer", role: "assistant" as const, text: "Notifications work." },
    ],
  };
  try {
    act(() =>
      root.render(
        createElement(AgentTabView, {
          title: "Worker",
          session,
          visible: true,
        }),
      ),
    );
    const buttons = container.querySelectorAll<HTMLButtonElement>(
      'button[aria-label="Save as note"]',
    );
    expect(buttons).toHaveLength(2);
    for (const button of buttons) await act(async () => button.click());
    for (const body of ["Check notifications", "Notifications work."]) {
      expect(invoke).toHaveBeenCalledWith("notes_upsert", {
        note: expect.objectContaining({
          body,
          title: "Inspect notifications",
          sourceSessionId: session.id,
          sourceCwd: session.cwd,
        }),
      });
    }
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});
