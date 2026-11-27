// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import {
  ARTIFACTS_CHANGED_EVENT,
  ARTIFACT_DELETED_EVENT,
  saveArtifact,
  type Artifact,
  type ArtifactUpsert,
} from "./artifacts";
import { usePersistedArtifacts } from "./usePersistedArtifacts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function deferredList() {
  let resolve!: (items: Artifact[]) => void;
  const promise = new Promise<Artifact[]>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}

it("removes a deleted document and shows it again when saved with the same ID", async () => {
  const draft: ArtifactUpsert = {
    id: "report",
    kind: "document",
    title: "Original",
    body: "",
  };
  const original = { ...draft, createdAt: 1, updatedAt: 1 };
  const other = { ...original, id: "other", title: "Other document" };
  const replacement = { ...draft, title: "Recreated", body: "New content" };
  const saved = { ...replacement, createdAt: 2, updatedAt: 2 };
  let onDelete!: EventCallback<string>;
  vi.mocked(listen).mockImplementation(async (_event, handler) => {
    onDelete = handler as EventCallback<string>;
    return () => {};
  });
  const beforeDelete = deferredList();
  const afterDelete = deferredList();
  vi.mocked(invoke)
    .mockResolvedValueOnce([original, other])
    .mockReturnValueOnce(beforeDelete.promise)
    .mockReturnValueOnce(afterDelete.promise)
    .mockResolvedValueOnce(saved)
    .mockResolvedValueOnce([saved, other]);

  function Documents() {
    return usePersistedArtifacts().map((item) => item.title).join(", ");
  }

  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  const root = createRoot(container);
  try {
    await act(async () => root.render(createElement(Documents)));
    expect(container.textContent).toBe("Original, Other document");
    expect(listen).toHaveBeenCalledWith(
      ARTIFACT_DELETED_EVENT,
      expect.any(Function),
    );

    act(() => {
      window.dispatchEvent(new Event(ARTIFACTS_CHANGED_EVENT));
    });
    await act(async () =>
      onDelete({ event: ARTIFACT_DELETED_EVENT, id: 1, payload: original.id }),
    );
    await act(async () => beforeDelete.resolve([original, other]));
    expect(container.textContent).toBe("Other document");

    await act(async () => {
      await saveArtifact(replacement);
    });
    expect(container.textContent).toBe("Recreated, Other document");

    await act(async () => afterDelete.resolve([other]));
    expect(container.textContent).toBe("Recreated, Other document");
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
