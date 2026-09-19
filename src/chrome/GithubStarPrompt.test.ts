// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  GithubStarPrompt,
  resetGithubStarPromptCacheForTest,
} from "./GithubStarPrompt";

const mocks = vi.hoisted(() => ({
  openUrl: vi.fn(),
  star: vi.fn(),
  starStatus: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: mocks.openUrl }));
vi.mock("../lib/githubTasks", () => ({
  githubMonocodeStarStatus: mocks.starStatus,
  starMonocodeOnGithub: mocks.star,
}));

let container: HTMLDivElement;
let root: Root;

function mockLocalStorage() {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
    clear: () => data.clear(),
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mockLocalStorage();
  resetGithubStarPromptCacheForTest();
  mocks.openUrl.mockReset();
  mocks.openUrl.mockResolvedValue(undefined);
  mocks.star.mockReset();
  mocks.star.mockResolvedValue(undefined);
  mocks.starStatus.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function renderPrompt(status: "starred" | "notStarred" | "unavailable") {
  mocks.starStatus.mockResolvedValue(status);
  await act(async () => root.render(createElement(GithubStarPrompt)));
}

it("stars MonoCode directly when the active account has not starred it", async () => {
  await renderPrompt("notStarred");

  const button = container.querySelector<HTMLButtonElement>(
    '[aria-label="Star MonoCode on GitHub"]',
  );
  expect(button?.textContent).toContain("Star on GitHub");

  await act(async () => button?.click());
  expect(mocks.star).toHaveBeenCalledOnce();
  expect(mocks.openUrl).not.toHaveBeenCalled();
  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();
});

it("falls back to GitHub when the authenticated API action fails", async () => {
  mocks.star.mockRejectedValue(new Error("missing write scope"));
  await renderPrompt("notStarred");

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Star MonoCode on GitHub"]',
      )
      ?.click(),
  );

  expect(mocks.openUrl).toHaveBeenCalledWith(
    "https://github.com/hardbeat920/monocode",
  );
  expect(container.querySelector("[data-github-star-prompt]")).not.toBeNull();
});

it("shows progress and ignores duplicate clicks while the star is pending", async () => {
  let finish!: () => void;
  mocks.star.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  await renderPrompt("notStarred");
  const button = container.querySelector<HTMLButtonElement>(
    '[aria-label="Star MonoCode on GitHub"]',
  )!;

  act(() => {
    button.click();
    button.click();
  });

  expect(mocks.star).toHaveBeenCalledOnce();
  expect(button.disabled).toBe(true);
  expect(button.textContent).toContain("Starring…");

  await act(async () => finish());
  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();
});

it("keeps its resolved state across rail remounts without checking again", async () => {
  await renderPrompt("notStarred");
  expect(mocks.starStatus).toHaveBeenCalledOnce();

  act(() => root.unmount());
  root = createRoot(container);
  act(() => root.render(createElement(GithubStarPrompt)));

  expect(container.querySelector("[data-github-star-prompt]")).not.toBeNull();
  expect(mocks.starStatus).toHaveBeenCalledOnce();
});

it("stays dismissed across remounts without checking GitHub again", async () => {
  await renderPrompt("notStarred");

  act(() => {
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Dismiss GitHub star prompt"]',
      )
      ?.click();
  });

  expect(localStorage.getItem("monocode.githubStarPrompt.dismissed.v1")).toBe(
    "1",
  );
  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();

  act(() => root.unmount());
  resetGithubStarPromptCacheForTest();
  mocks.starStatus.mockClear();
  root = createRoot(container);
  await act(async () => root.render(createElement(GithubStarPrompt)));

  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();
  expect(mocks.starStatus).not.toHaveBeenCalled();
});

it("stays out of the rail when the account is starred or cannot be checked", async () => {
  await renderPrompt("starred");
  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();

  act(() => root.unmount());
  resetGithubStarPromptCacheForTest();
  root = createRoot(container);
  await renderPrompt("unavailable");
  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();
});

it("rechecks after returning from GitHub and disappears once starred", async () => {
  await renderPrompt("notStarred");
  expect(container.querySelector("[data-github-star-prompt]")).not.toBeNull();

  mocks.starStatus.mockResolvedValue("starred");
  await act(async () => window.dispatchEvent(new Event("focus")));

  expect(container.querySelector("[data-github-star-prompt]")).toBeNull();
});
