// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteControlLink } from "./RemoteControlLink";
import type { BridgeStatus } from "../model/transcript";

const copied = vi.hoisted(() => ({ text: [] as string[], fail: false }));

vi.mock("../../../platform/tauri/clipboard", () => ({
  copyText: (text: string) => {
    if (copied.fail) return Promise.reject(new Error("no clipboard"));
    copied.text.push(text);
    return Promise.resolve();
  },
}));

const cues = vi.hoisted(() => ({ played: [] as string[] }));

vi.mock("../../settings/model/sounds", () => ({
  playCue: (cue: string) => cues.played.push(cue),
}));

const URL = "https://claude.ai/code/session_01V1ZQG948YQbhEq2nusnbMc";

let container: HTMLDivElement;
let root: Root;

function render(bridge: BridgeStatus | undefined) {
  act(() => {
    root.render(createElement(RemoteControlLink, { bridge }));
  });
}

function copyButton(): HTMLButtonElement | null {
  return container.querySelector<HTMLButtonElement>("button");
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  copied.text = [];
  copied.fail = false;
  cues.played = [];
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("RemoteControlLink", () => {
  it("shows the URL when remote control is active", () => {
    render({ active: true, url: URL });
    expect(container.textContent).toContain(URL);
  });

  it("renders nothing when remote control is not active", () => {
    render({ active: false });
    expect(container.textContent).toBe("");
    expect(copyButton()).toBeNull();
  });

  it("renders nothing when there is no bridge at all", () => {
    render(undefined);
    expect(container.textContent).toBe("");
  });

  it("keeps a stale URL out of sight once it is no longer active", () => {
    // A closed bridge must not leave a link that looks usable.
    render({ active: false, url: URL });
    expect(container.textContent).toBe("");
  });

  it("says it is starting when active before the link has arrived", () => {
    // The bridge_status record lands shortly after the pty starts.
    render({ active: true });
    expect(container.textContent).toMatch(/starting/i);
    expect(container.textContent).not.toContain("http");
    // Nothing to copy yet, so no button to press.
    expect(copyButton()).toBeNull();
  });

  it("copies the URL and confirms it", async () => {
    render({ active: true, url: URL });
    const button = copyButton()!;
    expect(button.getAttribute("aria-label")).toBe(
      "Copy remote control link",
    );

    await act(async () => {
      button.click();
    });

    expect(copied.text).toEqual([URL]);
    expect(cues.played).toEqual(["copy"]);
    expect(copyButton()!.getAttribute("aria-label")).toBe("Copied");
  });

  it("copies the link exactly, appending nothing", async () => {
    // There is no `/rc` suffix — what looks like one on screen is the TUI's
    // own status-line indicator, not part of the link.
    render({ active: true, url: URL });
    await act(async () => {
      copyButton()!.click();
    });
    expect(copied.text[0]).toBe(URL);
    expect(copied.text[0]).not.toMatch(/\/rc$/);
  });

  it("stays quiet when the clipboard refuses", async () => {
    copied.fail = true;
    render({ active: true, url: URL });

    await act(async () => {
      copyButton()!.click();
    });

    // No confirmation for something that did not happen, and no throw.
    expect(cues.played).toEqual([]);
    expect(copyButton()!.getAttribute("aria-label")).toBe(
      "Copy remote control link",
    );
  });

  it("drops the copied confirmation when the URL changes", async () => {
    render({ active: true, url: URL });
    await act(async () => {
      copyButton()!.click();
    });
    expect(copyButton()!.getAttribute("aria-label")).toBe("Copied");

    render({ active: true, url: `${URL}-other` });

    expect(copyButton()!.getAttribute("aria-label")).toBe(
      "Copy remote control link",
    );
  });

  it("survives the same URL across a handover", () => {
    // The URL belongs to the conversation, so a resume re-renders the same one
    // and must keep showing it rather than treating it as per-launch state.
    render({ active: true, url: URL });
    render({ active: true, url: URL });
    expect(container.textContent).toContain(URL);
  });
});
