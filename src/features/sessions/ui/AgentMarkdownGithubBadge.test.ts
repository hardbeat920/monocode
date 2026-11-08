// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

const actions = vi.hoisted(() => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: actions.openUrl,
}));

describe("GitHub reference badge", () => {
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

  async function render(text: string) {
    await act(async () =>
      root.render(createElement(AgentMarkdown, { text, cwd: "/repo" })),
    );
  }

  it("renders a badge with the GitHub mark for a pull request link", async () => {
    await render(
      "See [PR 1343](https://github.com/hardbeat920/monocode/pull/1343).",
    );
    const link = container.querySelector<HTMLAnchorElement>(
      'a[href="https://github.com/hardbeat920/monocode/pull/1343"]',
    );
    expect(link).not.toBeNull();
    expect(link!.querySelector("svg")).not.toBeNull();
    expect(link!.textContent).toContain("#1343");
  });

  it("shortens a commit SHA in the badge label", async () => {
    await render(
      "Fixed in https://github.com/acme/widgets/commit/0123456789abcdef0123456789abcdef01234567",
    );
    const link = container.querySelector<HTMLAnchorElement>("a");
    expect(link).not.toBeNull();
    expect(link!.textContent).toContain("0123456");
  });

  it("opens the original link when the badge is clicked", async () => {
    await render("[#7](https://github.com/acme/widgets/issues/7)");
    const link = container.querySelector<HTMLAnchorElement>("a")!;
    await act(async () => link.click());
    expect(actions.openUrl).toHaveBeenCalledWith(
      "https://github.com/acme/widgets/issues/7",
    );
  });

  it("leaves ordinary web links untouched", async () => {
    await render("[docs](https://example.com/guide)");
    const link = container.querySelector<HTMLAnchorElement>("a")!;
    expect(link.querySelector("svg")).toBeNull();
    expect(link.textContent).toBe("docs");
  });
});
