// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

const EXPIRED =
  "Failed to authenticate: OAuth session expired and could not be refreshed";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function failedTurn(reply: Partial<Block> = {}): Block[] {
  return [
    { id: "prompt", role: "user", text: "Fix the build" },
    { id: "reply", role: "assistant", text: EXPIRED, ...reply },
  ];
}

function button(label: string): HTMLButtonElement | undefined {
  return [...container.querySelectorAll<HTMLButtonElement>("button")].find(
    (item) => item.textContent?.trim() === label,
  );
}

function notice(): Element | null {
  return container.querySelector("[data-provider-sign-in]");
}

describe("AgentTranscript provider sign-in", () => {
  it("offers sign-in under the failed turn and a retry once it succeeds", async () => {
    let finish!: () => void;
    const signIn = vi.fn(
      () => new Promise<void>((resolve) => (finish = resolve)),
    );
    const retry = vi.fn();
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: failedTurn(),
          harness: "claude",
          onProviderSignIn: signIn,
          onRetryAfterSignIn: retry,
        }),
      ),
    );

    expect(notice()?.textContent).toContain(
      "Sign in to Claude Code to continue",
    );
    // The provider's message stays readable, once, inside the notice.
    expect(notice()?.textContent).toContain(EXPIRED);
    expect(container.textContent?.split(EXPIRED)).toHaveLength(2);
    expect(button("Retry")).toBeUndefined();

    act(() => button("Sign in")!.click());
    expect(signIn).toHaveBeenCalledOnce();
    expect(button("Waiting for browser…")?.disabled).toBe(true);

    await act(async () => finish());
    expect(notice()?.textContent).toContain("Signed in to Claude Code");
    act(() => button("Retry")!.click());
    expect(retry).toHaveBeenCalledOnce();
  });

  it("shows why sign-in failed and lets it be tried again", async () => {
    const signIn = vi.fn(async () => {
      throw new Error("Claude Code sign-in timed out. Please try again.");
    });
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: failedTurn(),
          harness: "claude",
          onProviderSignIn: signIn,
        }),
      ),
    );

    await act(async () => button("Sign in")!.click());
    expect(notice()?.textContent).toContain(
      "Claude Code sign-in timed out. Please try again.",
    );
    expect(button("Sign in")?.disabled).toBe(false);
  });

  it("replaces a provider sign-in error for any harness that can sign in", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: [
            { id: "prompt", role: "user", text: "Hello" },
            {
              id: "error",
              role: "system",
              notice: "error",
              text: "Grok Build is not signed in. Run `grok login` in a terminal.",
            },
          ],
          harness: "grok",
          onProviderSignIn: vi.fn(async () => undefined),
        }),
      ),
    );
    expect(notice()?.textContent).toContain(
      "Sign in to Grok Build to continue",
    );
  });

  it("leaves ordinary replies and sessions without sign-in alone", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: failedTurn({
            text: "Failed to authenticate errors usually mean the token in your .env is stale.",
          }),
          harness: "claude",
          onProviderSignIn: vi.fn(async () => undefined),
        }),
      ),
    );
    expect(notice()).toBeNull();

    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: failedTurn(),
          harness: "claude",
        }),
      ),
    );
    expect(notice()).toBeNull();
    expect(container.textContent).toContain(EXPIRED);
  });

  it("waits for the turn to settle", () => {
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: failedTurn({ streaming: true }),
          harness: "claude",
          busy: true,
          onProviderSignIn: vi.fn(async () => undefined),
        }),
      ),
    );
    expect(notice()).toBeNull();
  });
});
