// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useTranscriptSelection } from "./useTranscriptSelection";
import type { TranscriptSelection } from "../model/transcriptSelection";

let container: HTMLDivElement;
let root: Root;
let latest: TranscriptSelection | null = null;
let response: HTMLParagraphElement;

function Probe({ host }: { host: HTMLElement }) {
  latest = useTranscriptSelection(host, true).selection;
  return null;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  latest = null;
  container = document.createElement("div");
  response = document.createElement("p");
  response.setAttribute("data-selectable-agent-response", "r1");
  response.textContent = "Select this sentence";
  container.append(response);
  document.body.append(container);
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  window.getSelection()!.removeAllRanges();
  vi.unstubAllGlobals();
});

function select() {
  const range = document.createRange();
  range.selectNodeContents(response);
  const rects = [new DOMRect(10, 20, 100, 20)];
  vi.spyOn(range, "getClientRects").mockReturnValue(
    Object.assign(rects, {
      item: (index: number) => rects[index] ?? null,
    }) as unknown as DOMRectList,
  );
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
}
const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));

it("anchors the menu where the pointer let go of the mouse", async () => {
  act(() => root.render(createElement(Probe, { host: container })));
  select();
  await act(async () => {
    response.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    document.dispatchEvent(
      new MouseEvent("pointerup", { clientX: 240, clientY: 90 }),
    );
    await frame();
    // The browser also reports the settled selection after the pointer lifts.
    document.dispatchEvent(new Event("selectionchange"));
    await frame();
  });
  expect(latest?.atPointer).toBe(true);
  expect(latest?.rect.left).toBe(240);
  expect(latest?.rect.top).toBe(90);
  expect(latest?.text).toBe("Select this sentence");
});

it("falls back to the selected text when the keyboard made the selection", async () => {
  act(() => root.render(createElement(Probe, { host: container })));
  select();
  await act(async () => {
    document.dispatchEvent(new Event("selectionchange"));
    await frame();
  });
  expect(latest?.atPointer).toBeFalsy();
  expect(latest?.rect.left).toBe(10);
});
