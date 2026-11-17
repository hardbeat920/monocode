// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it } from "vitest";
import {
  cancelMonoScrollRestore,
  readMonoScroll,
  restoreMonoScroll,
} from "./monoScroll";

let frames: (() => void)[];
const options = {
  frame: (callback: () => void) => frames.push(callback),
  cancelFrame: () => {},
};
const step = () => frames.splice(0).forEach((callback) => callback());

/** Layout is not computed here, so a scroller's room is set by hand. */
function scroller(attrs: Record<string, string>, top = 0, room = 1000) {
  const el = document.createElement("div");
  for (const [name, value] of Object.entries(attrs)) {
    el.setAttribute(name, value);
  }
  Object.defineProperty(el, "scrollHeight", {
    get: () => room,
    configurable: true,
  });
  Object.defineProperty(el, "clientHeight", { value: 100 });
  el.scrollTop = top;
  document.body.append(el);
  return el;
}
const overview = (top = 0, room = 1000, tab = "documents") =>
  scroller(
    { "data-mono-scroll": "overview", "data-mono-overview": tab },
    top,
    room,
  );
const page = (top = 0, room = 1000) =>
  scroller({ "data-mono-scroll": "document" }, top, room);

beforeEach(() => {
  frames = [];
});
afterEach(() => {
  cancelMonoScrollRestore();
  document.body.replaceChildren();
});

it("reads where the overview and an open document were scrolled", () => {
  overview(240);
  page(90);
  expect(readMonoScroll()).toEqual({
    overview: { tab: "documents", top: 240 },
    document: { top: 90 },
  });
});

it("reads nothing from an unscrolled or hidden overview", () => {
  overview(0);
  expect(readMonoScroll()).toBeUndefined();
  document.body.replaceChildren();
  overview(50).hidden = true;
  expect(readMonoScroll()).toBeUndefined();
});

it("puts the scroll back once a loading document has room for it", () => {
  restoreMonoScroll(
    { overview: { tab: "documents", top: 240 }, document: { top: 90 } },
    options,
  );
  const list = overview();
  step(); // the document has not mounted yet
  expect(list.scrollTop).toBe(240);
  const doc = page(0, 50); // mounted, still loading
  step();
  expect(doc.scrollTop).toBe(0);
  Object.defineProperty(doc, "scrollHeight", { value: 1000 });
  step();
  expect(doc.scrollTop).toBe(90);
  expect(frames).toHaveLength(0);
});

it("leaves another tab's list, and a document that never comes, alone", () => {
  restoreMonoScroll(
    { overview: { tab: "documents", top: 240 }, document: { top: 90 } },
    { ...options, frames: 3 },
  );
  const other = overview(0, 1000, "habits");
  for (let n = 0; n < 5; n++) step();
  expect(other.scrollTop).toBe(0);
  expect(frames).toHaveLength(0);
});

it("settles for what fits when content never grows enough", () => {
  const list = overview(0, 150);
  restoreMonoScroll(
    { overview: { tab: "documents", top: 240 } },
    { ...options, frames: 2 },
  );
  step();
  expect(list.scrollTop).toBe(0);
  step();
  expect(list.scrollTop).toBeGreaterThan(0);
});

it("stops when the reader scrolls, or another Mono opens", () => {
  const list = overview(0, 150);
  restoreMonoScroll({ overview: { tab: "documents", top: 100 } }, options);
  document.dispatchEvent(new Event("wheel", { bubbles: true }));
  Object.defineProperty(list, "scrollHeight", { value: 1000 });
  step();
  expect(list.scrollTop).toBe(0);

  restoreMonoScroll({ overview: { tab: "documents", top: 100 } }, options);
  cancelMonoScrollRestore();
  step();
  expect(list.scrollTop).toBe(0);
});
