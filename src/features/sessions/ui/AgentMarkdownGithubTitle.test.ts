// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

const backend = vi.hoisted(() => ({
  githubStatus: vi.fn(async () => ({
    connected: true,
    installed: true,
    authenticated: true,
  })),
  peekGithubWorkItem: vi.fn(() => null),
  githubWorkItem: vi.fn(async () => ({
    kind: "pr" as const,
    number: 1343,
    title: "Mobile: paste a copied image into the composer",
    url: "https://github.com/hardbeat920/monocode/pull/1343",
    state: "merged",
    updatedAt: "",
    labels: [],
    assignees: [],
    draft: false,
    repo: "hardbeat920/monocode",
  })),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

vi.mock("../../inbox/model/githubTasks", () => backend);

describe("GitHub badge title enrichment", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.clearAllMocks();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  async function flush() {
    for (let i = 0; i < 10; i++) {
      await act(async () => {
        await Promise.resolve();
      });
    }
  }

  it("appends the fetched PR title to the badge", async () => {
    await act(async () =>
      root.render(
        createElement(AgentMarkdown, {
          text: "[PR](https://github.com/hardbeat920/monocode/pull/1343)",
          cwd: "/repo",
        }),
      ),
    );
    await flush();

    const link = container.querySelector<HTMLAnchorElement>("a")!;
    expect(link.textContent).toContain("#1343");
    // Title is clamped to six words and revealed on hover.
    expect(link.textContent).toContain("Mobile: paste a copied image into…");
    // The full title stays available as the tooltip.
    expect(link.getAttribute("title")).toContain(
      "Mobile: paste a copied image into the composer",
    );
    expect(backend.githubWorkItem).toHaveBeenCalledWith(
      "/repo",
      "hardbeat920/monocode",
      "pr",
      1343,
    );
  });

  it("does not fetch a title for commit links", async () => {
    await act(async () =>
      root.render(
        createElement(AgentMarkdown, {
          text: "https://github.com/acme/widgets/commit/0123456789abcdef0123456789abcdef01234567",
          cwd: "/repo",
        }),
      ),
    );
    await flush();

    expect(backend.githubWorkItem).not.toHaveBeenCalled();
    const link = container.querySelector<HTMLAnchorElement>("a")!;
    expect(link.textContent).toContain("0123456");
  });
});
