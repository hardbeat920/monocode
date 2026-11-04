// @vitest-environment happy-dom
// Keep this as .ts because the project test glob intentionally excludes .test.tsx.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({
  loginHarness:
    vi.fn<
      (
        _harness: string,
        _account?: string,
        _options?: { signal?: AbortSignal; onUrl?: (url: string) => void },
      ) => Promise<void>
    >(),
  submitHarnessLoginCode: vi.fn(async () => undefined),
}));

vi.mock("../../../integrations/harness/core/auth", () => ({
  loginHarness: auth.loginHarness,
  submitHarnessLoginCode: auth.submitHarnessLoginCode,
}));
const clipboard = vi.hoisted(() => ({
  copyText: vi.fn(async () => undefined),
}));
vi.mock("../../../platform/tauri/clipboard", () => clipboard);

import { ProviderSignInDialog } from "./ProviderSignInDialog";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  auth.loginHarness.mockReset();
  auth.submitHarnessLoginCode.mockReset();
  auth.submitHarnessLoginCode.mockResolvedValue();
  clipboard.copyText.mockReset();
  clipboard.copyText.mockResolvedValue();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function button(label: string): HTMLButtonElement {
  const result = [
    ...document.querySelectorAll<HTMLButtonElement>("button"),
  ].find(
    (item) => (item.getAttribute("aria-label") ?? item.textContent) === label,
  );
  expect(result, label).toBeDefined();
  return result!;
}

describe("ProviderSignInDialog", () => {
  it("launches provider login and offers a clear continuation state", async () => {
    const close = vi.fn();
    auth.loginHarness.mockResolvedValue();
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "grok",
          onClose: close,
        }),
      ),
    );

    const dialog = document.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain("Authentication required");
    expect(dialog?.textContent).toContain("Sign in to Grok Build");
    expect(dialog?.querySelector(".size-9")).not.toBeNull();

    await act(async () => button("Sign in to Grok Build").click());
    expect(auth.loginHarness).toHaveBeenCalledWith(
      "grok",
      undefined,
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(dialog?.textContent).toContain("Signed in to Grok Build");

    act(() => button("Continue").click());
    expect(close).toHaveBeenCalledOnce();
  });

  it("cancels a waiting login, retries, and cancels on close", async () => {
    const close = vi.fn();
    auth.loginHarness.mockImplementation(
      (_harness, _account, options) =>
        new Promise<void>((_resolve, reject) =>
          options?.signal?.addEventListener("abort", () =>
            reject(new Error("Sign-in cancelled.")),
          ),
        ),
    );
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "claude",
          onClose: close,
        }),
      ),
    );
    act(() => button("Sign in to Claude Code").click());
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Waiting for browser…",
    );
    await act(async () => button("Cancel sign-in").click());
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Sign in to Claude Code",
    );
    act(() => button("Sign in to Claude Code").click());
    expect(auth.loginHarness).toHaveBeenCalledTimes(2);
    await act(async () => button("Close").click());
    expect(close).toHaveBeenCalledOnce();
    expect(auth.loginHarness.mock.calls[1]?.[2]?.signal?.aborted).toBe(true);
  });

  it("offers Claude's printed link for another browser and accepts its code", async () => {
    auth.loginHarness.mockImplementation(
      () => new Promise<void>(() => undefined),
    );
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "claude",
          providerAccountId: "account-work",
          onClose: vi.fn(),
        }),
      ),
    );
    act(() => button("Sign in to Claude Code").click());
    act(() =>
      auth.loginHarness.mock.calls[0]?.[2]?.onUrl?.(
        "https://claude.ai/oauth/authorize?state=private",
      ),
    );
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "another browser",
    );
    await act(async () => button("Copy sign-in link").click());
    expect(clipboard.copyText).toHaveBeenCalledWith(
      "https://claude.ai/oauth/authorize?state=private",
    );
    const input =
      document.querySelector<HTMLInputElement>('input[name="code"]')!;
    act(() => {
      input.value = "sample-code";
      input
        .closest("form")
        ?.dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        );
    });
    expect(auth.submitHarnessLoginCode).toHaveBeenCalledWith(
      "claude",
      "sample-code",
      auth.loginHarness.mock.calls[0]?.[2]?.signal,
      "account-work",
    );
    expect(auth.loginHarness.mock.calls[0]?.[1]).toBe("account-work");
  });

  it("ignores late copy and code errors from a cancelled attempt after retry", async () => {
    let rejectCopy!: (error: Error) => void;
    let rejectCode!: (error: Error) => void;
    clipboard.copyText.mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectCopy = reject;
        }),
    );
    auth.submitHarnessLoginCode.mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectCode = reject;
        }),
    );
    auth.loginHarness.mockImplementation(
      (_harness, _account, options) =>
        new Promise<void>((_resolve, reject) =>
          options?.signal?.addEventListener("abort", () =>
            reject(new Error("Sign-in cancelled.")),
          ),
        ),
    );
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "claude",
          onClose: vi.fn(),
        }),
      ),
    );
    act(() => button("Sign in to Claude Code").click());
    act(() =>
      auth.loginHarness.mock.calls[0]?.[2]?.onUrl?.(
        "https://claude.ai/oauth/authorize?state=old",
      ),
    );
    act(() => button("Copy sign-in link").click());
    const input =
      document.querySelector<HTMLInputElement>('input[name="code"]')!;
    act(() => {
      input.value = "old-code";
      input
        .closest("form")
        ?.dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        );
    });
    await act(async () => button("Cancel sign-in").click());
    act(() => button("Sign in to Claude Code").click());
    act(() =>
      auth.loginHarness.mock.calls[1]?.[2]?.onUrl?.(
        "https://claude.ai/oauth/authorize?state=new",
      ),
    );
    await act(async () => {
      rejectCopy(new Error("old clipboard failure"));
      rejectCode(new Error("old code failure"));
    });
    expect(
      document.querySelector('[role="dialog"]')?.textContent,
    ).not.toContain("Could not");
    clipboard.copyText.mockRejectedValueOnce(
      new Error("current clipboard failure"),
    );
    await act(async () => button("Copy sign-in link").click());
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Could not copy sign-in link.",
    );
    auth.submitHarnessLoginCode.mockRejectedValueOnce(
      new Error("current code failure"),
    );
    const retryInput =
      document.querySelector<HTMLInputElement>('input[name="code"]')!;
    await act(async () => {
      retryInput.value = "current-code";
      retryInput
        .closest("form")
        ?.dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        );
    });
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Could not submit sign-in code.",
    );
  });

  it("ignores a late copy failure after the account changes", async () => {
    let rejectCopy!: (error: Error) => void;
    clipboard.copyText.mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectCopy = reject;
        }),
    );
    auth.loginHarness.mockImplementation(
      () => new Promise<void>(() => undefined),
    );
    const render = (providerAccountId: string) =>
      act(() =>
        root.render(
          createElement(ProviderSignInDialog, {
            harness: "claude",
            providerAccountId,
            onClose: vi.fn(),
          }),
        ),
      );
    render("account-old");
    act(() => button("Sign in to Claude Code").click());
    act(() =>
      auth.loginHarness.mock.calls[0]?.[2]?.onUrl?.(
        "https://claude.ai/oauth/authorize?state=old",
      ),
    );
    act(() => button("Copy sign-in link").click());
    render("account-new");
    act(() => button("Sign in to Claude Code").click());
    await act(async () => rejectCopy(new Error("old clipboard failure")));
    expect(
      document.querySelector('[role="dialog"]')?.textContent,
    ).not.toContain("Could not copy");
  });

  it("keeps a completed sign-in clear of a late copy failure", async () => {
    let rejectCopy!: (error: Error) => void;
    let finishLogin!: () => void;
    clipboard.copyText.mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectCopy = reject;
        }),
    );
    auth.loginHarness.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishLogin = resolve;
        }),
    );
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "claude",
          onClose: vi.fn(),
        }),
      ),
    );
    act(() => button("Sign in to Claude Code").click());
    act(() =>
      auth.loginHarness.mock.calls[0]?.[2]?.onUrl?.(
        "https://claude.ai/oauth/authorize?state=old",
      ),
    );
    act(() => button("Copy sign-in link").click());
    await act(async () => finishLogin());
    await act(async () => rejectCopy(new Error("late clipboard failure")));
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Signed in to Claude Code",
    );
    expect(
      document.querySelector('[role="dialog"]')?.textContent,
    ).not.toContain("Could not copy");
  });

  it("signs a named Codex session into its own account", async () => {
    auth.loginHarness.mockResolvedValue();
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "codex",
          providerAccountId: "account-codex",
          onClose: vi.fn(),
        }),
      ),
    );
    await act(async () => button("Sign in to Codex").click());
    expect(auth.loginHarness.mock.calls[0]?.[1]).toBe("account-codex");
  });

  it("keeps provider errors visible and allows a retry", async () => {
    auth.loginHarness.mockRejectedValueOnce(new Error("Sign-in failed"));
    auth.loginHarness.mockResolvedValueOnce();
    act(() =>
      root.render(
        createElement(ProviderSignInDialog, {
          harness: "claude",
          onClose: vi.fn(),
        }),
      ),
    );
    await act(async () => button("Sign in to Claude Code").click());
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Sign-in failed",
    );
    await act(async () => button("Sign in to Claude Code").click());
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain(
      "Signed in to Claude Code",
    );
  });
});
