// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { copyMessage } from "../../../platform/tauri/clipboard";
import { ARTIFACTS_CHANGED_EVENT, type Artifact } from "../artifacts";
import { ArtifactPanel } from "./ArtifactPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../sessions/ui/AgentMarkdown", () => ({
  AgentMarkdown: ({ text }: { text: string }) =>
    createElement("div", null, text),
}));
vi.mock("../../../platform/tauri/clipboard", () => ({
  copyMessage: vi.fn().mockResolvedValue(undefined),
}));

const report: Artifact = {
  id: "doc-report",
  kind: "document",
  title: "PR review",
  body: "Detailed report",
  createdAt: 1,
  updatedAt: 2,
};
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockReset();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("loads document artifacts through their own command and closes with Escape", async () => {
  vi.mocked(invoke).mockResolvedValue(report);
  const onClose = vi.fn();
  await act(async () =>
    root.render(
      createElement(ArtifactPanel, { id: report.id, color: "#aaa", onClose }),
    ),
  );
  expect(invoke).toHaveBeenCalledWith("artifacts_get", { id: report.id });
  expect(container.textContent).toContain(report.body);
  expect(container.querySelector("[data-mono-artifact]")).not.toBeNull();
  const copy = container.querySelector<HTMLButtonElement>(
    'header [aria-label="Copy document"]',
  )!;
  expect(copy.nextElementSibling?.getAttribute("aria-label")).toBe(
    "Hide document",
  );
  expect(
    container.querySelector('article [aria-label="Copy document"]'),
  ).toBeNull();
  await act(async () => copy.click());
  expect(copyMessage).toHaveBeenCalledWith(report.body);
  act(() =>
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })),
  );
  expect(onClose).toHaveBeenCalledOnce();
});

it("keeps a revised document when an older read finishes after refresh", async () => {
  let finish!: (value: Artifact) => void;
  vi.mocked(invoke).mockImplementationOnce(
    () =>
      new Promise<Artifact>((resolve) => {
        finish = resolve;
      }),
  );
  act(() =>
    root.render(
      createElement(ArtifactPanel, {
        id: report.id,
        color: "#aaa",
        onClose: vi.fn(),
      }),
    ),
  );
  vi.mocked(invoke).mockResolvedValue({ ...report, body: "Revised report" });
  await act(async () =>
    window.dispatchEvent(new Event(ARTIFACTS_CHANGED_EVENT)),
  );
  await act(async () => finish(report));
  expect(container.textContent).toContain("Revised report");
  expect(container.textContent).not.toContain("Detailed report");
});

it("shows missing documents without falling back to Notes", async () => {
  vi.mocked(invoke).mockResolvedValue(null);
  await act(async () =>
    root.render(
      createElement(ArtifactPanel, {
        id: report.id,
        color: "#aaa",
        onClose: vi.fn(),
      }),
    ),
  );
  expect(container.textContent).toContain("no longer available");
  expect(invoke).toHaveBeenCalledTimes(1);
});

it("can cancel deletion, keeps the reader on failure, and closes after a successful retry", async () => {
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  const onClose = vi.fn();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "artifacts_delete") throw new Error("Storage failed");
    return report;
  });
  await act(async () =>
    root.render(
      createElement(ArtifactPanel, {
        id: report.id,
        color: "#aaa",
        onClose,
      }),
    ),
  );
  const button = container.querySelector<HTMLButtonElement>(
    'header [aria-label="Delete document"]',
  )!;
  await act(async () => button.click());
  expect(confirm).toHaveBeenCalledWith(`Delete “${report.title}”?`);
  expect(invoke).not.toHaveBeenCalledWith("artifacts_delete", {
    id: report.id,
  });
  confirm.mockReturnValue(true);
  await act(async () => button.click());
  expect(container.textContent).toContain("Could not delete this document.");
  expect(container.textContent).toContain(report.body);
  expect(onClose).not.toHaveBeenCalled();
  vi.mocked(invoke).mockResolvedValue(undefined);
  await act(async () => button.click());
  expect(invoke).toHaveBeenCalledWith("artifacts_delete", { id: report.id });
  expect(onClose).toHaveBeenCalledOnce();
});
