// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  captureSessionListLayout,
  liftSessionGhost,
  playSessionListLayout,
} from "./sessionDragMotion";

function rect(top: number, left = 0): DOMRect {
  return {
    top,
    left,
    right: left + 200,
    bottom: top + 40,
    width: 200,
    height: 40,
    x: left,
    y: top,
    toJSON: () => ({}),
  };
}

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("liftSessionGhost", () => {
  function card() {
    const el = document.createElement("div");
    el.dataset.sessionCard = "session-1";
    el.className = "opacity-40";
    el.innerHTML = '<div role="button" data-session-select="session-1">Title</div>';
    el.getBoundingClientRect = () => rect(100, 10);
    document.body.appendChild(el);
    return el;
  }

  function ghostEl() {
    return document.body.lastElementChild as HTMLElement;
  }

  it("lifts an inert copy that hit-testing and assistive tech ignore", () => {
    liftSessionGhost(card(), 30, 110);
    const ghost = ghostEl();
    expect(ghost.style.pointerEvents).toBe("none");
    expect(ghost.getAttribute("aria-hidden")).toBe("true");
    expect(ghost.querySelector("[data-session-select],[role]")).toBeNull();
    expect(ghost.hasAttribute("data-session-card")).toBe(false);
    expect(ghost.classList.contains("opacity-40")).toBe(false);
    expect(ghost.textContent).toBe("Title");
  });

  it("follows the pointer from where the card was grabbed", () => {
    const ghost = liftSessionGhost(card(), 30, 110);
    ghost.move(80, 150);
    expect(ghostEl().style.transform).toContain("translate(50px, 40px)");
  });

  it("removes itself on cancel, and on a drop with nowhere visible to land", () => {
    vi.useFakeTimers();
    const first = liftSessionGhost(card(), 30, 110);
    const firstEl = ghostEl();
    first.cancel();
    vi.advanceTimersByTime(1000);
    expect(firstEl.isConnected).toBe(false);

    const second = liftSessionGhost(card(), 30, 110);
    const secondEl = ghostEl();
    second.land(null);
    expect(secondEl.isConnected).toBe(false);
  });
});

describe("playSessionListLayout", () => {
  function row(attr: "sessionCard" | "sessionFolder", id: string, top: number) {
    const el = document.createElement("div");
    el.dataset[attr] = id;
    el.getBoundingClientRect = () => rect(top);
    el.animate = vi.fn() as unknown as HTMLElement["animate"];
    return el;
  }

  it("slides moved rows from where they were, minus what their folder carries", () => {
    const list = document.createElement("ul");
    const folder = row("sessionFolder", "folder-1", 0);
    const inside = row("sessionCard", "inside", 20);
    const below = row("sessionCard", "below", 100);
    folder.appendChild(inside);
    list.append(folder, below);
    document.body.appendChild(list);
    const before = captureSessionListLayout(list);

    folder.getBoundingClientRect = () => rect(40);
    inside.getBoundingClientRect = () => rect(60);
    below.getBoundingClientRect = () => rect(140);
    playSessionListLayout(list, before);

    expect(folder.animate).toHaveBeenCalledWith(
      [{ transform: "translate(0px, -40px)" }, { transform: "none" }],
      expect.any(Object),
    );
    // Moved exactly as far as its folder did, so it rides along untouched.
    expect(inside.animate).not.toHaveBeenCalled();
    expect(below.animate).toHaveBeenCalledWith(
      [{ transform: "translate(0px, -40px)" }, { transform: "none" }],
      expect.any(Object),
    );
  });

  it("grows in rows that are new and leaves the dropped card to the ghost", () => {
    const list = document.createElement("ul");
    const dropped = row("sessionCard", "dropped", 0);
    list.appendChild(dropped);
    document.body.appendChild(list);
    const before = captureSessionListLayout(list);

    const created = row("sessionFolder", "folder-new", 0);
    list.prepend(created);
    dropped.getBoundingClientRect = () => rect(40);
    playSessionListLayout(list, before, "dropped");

    expect(created.animate).toHaveBeenCalledTimes(1);
    expect(dropped.animate).not.toHaveBeenCalled();
  });

  it("does nothing when the viewer prefers reduced motion", () => {
    vi.spyOn(window, "matchMedia").mockReturnValue({
      matches: true,
    } as MediaQueryList);
    const list = document.createElement("ul");
    const moved = row("sessionCard", "moved", 0);
    list.appendChild(moved);
    const before = captureSessionListLayout(list);
    moved.getBoundingClientRect = () => rect(80);
    playSessionListLayout(list, before);
    expect(moved.animate).not.toHaveBeenCalled();
  });
});
