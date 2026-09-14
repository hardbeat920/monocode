// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

describe("markdown file navigation", () => {
  let root: Root;
  let container: HTMLDivElement;
  const onOpenFile = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    onOpenFile.mockClear();
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
      root.render(
        createElement(AgentMarkdown, {
          text,
          cwd: "/repo",
          onOpenFile,
        }),
      ),
    );
  }

  it("keeps protocol methods and ordinary identifiers as code, not file chips", async () => {
    await render("`currentTime/read` and `experimentalApi` and `true`");
    for (const code of container.querySelectorAll("code")) {
      expect(code.getAttribute("role")).toBeNull();
      expect(code.querySelector('[aria-hidden="true"]')).toBeNull();
      await act(async () => code.click());
    }
    expect(onOpenFile).not.toHaveBeenCalled();
  });

  it.each(["Dockerfile", "Makefile", "Gemfile", "LICENSE", ".gitignore"])(
    "opens the extensionless/dotfile reference %s",
    async (name) => {
      await render(`\`${name}\``);
      const link = container.querySelector<HTMLElement>('code[role="link"]');
      expect(link).not.toBeNull();
      await act(async () => link!.click());
      expect(onOpenFile.mock.calls).toHaveLength(1);
      expect(onOpenFile.mock.calls[0][0]).toBe(`/repo/${name}`);
    },
  );

  it("opens inline file references at their line and column with the keyboard", async () => {
    await render("`src/main.ts:12:3`");
    const link = container.querySelector<HTMLElement>('code[role="link"]')!;
    await act(async () =>
      link.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      ),
    );
    expect(onOpenFile).toHaveBeenCalledWith("/repo/src/main.ts", {
      line: 12,
      column: 3,
    });
  });

  it.each([
    ["docs/my%20file.md", "/repo/docs/my file.md", undefined],
    [
      "docs/my%20file.md:12:3",
      "/repo/docs/my file.md",
      { line: 12, column: 3 },
    ],
    ["docs/progress%25.md", "/repo/docs/progress%.md", undefined],
  ] as const)(
    "opens the encoded inline file reference %s",
    async (reference, path, navigation) => {
      await render(`\`${reference}\``);
      const link = container.querySelector<HTMLElement>('code[role="link"]');
      expect(link).not.toBeNull();
      await act(async () => link!.click());
      expect(onOpenFile).toHaveBeenCalledWith(path, navigation);
    },
  );

  it("decodes spaces in markdown file links and preserves the source line", async () => {
    await render("[Guide](<docs/My Guide.md#L7-L9>)");
    expect(container.innerHTML).toContain("<a");
    await act(async () =>
      container.querySelector<HTMLAnchorElement>("a")!.click(),
    );
    expect(onOpenFile).toHaveBeenCalledWith("/repo/docs/My Guide.md", {
      line: 7,
    });
  });

  it.each(["`main.ts:12`", "[Source](main.ts:12)"])(
    "opens a bare filename with a line number: %s",
    async (text) => {
      await render(text);
      const link = container.querySelector<HTMLElement>('code[role="link"], a');
      expect(link).not.toBeNull();
      await act(async () => link!.click());
      expect(onOpenFile).toHaveBeenCalledWith("/repo/main.ts", { line: 12 });
    },
  );

  it.each([
    "/%2Fhost/share/file.md",
    "%2F%2Fhost/share/file.md:12",
    "%5C%5Chost/share/file.md",
  ])(
    "does not open an encoded network path as a local file: %s",
    async (href) => {
      await render(`[Source](${href})`);
      for (const link of container.querySelectorAll<HTMLAnchorElement>("a")) {
        await act(async () => link.click());
      }
      expect(onOpenFile).not.toHaveBeenCalled();
    },
  );

  it.each([
    ["./docs/guide.md#installation", "/repo/docs/guide.md"],
    ["./report%23L2.md#installation", "/repo/report#L2.md"],
  ])(
    "opens %s without treating the heading anchor as part of its filename",
    async (href, path) => {
      await render(`[Guide](${href})`);
      await act(async () =>
        container.querySelector<HTMLAnchorElement>("a")!.click(),
      );
      expect(onOpenFile).toHaveBeenCalledWith(path, undefined);
    },
  );

  it("opens absolute file URLs at the referenced line", async () => {
    await render("[Source](file:///Users/me/My%20Project/main.ts#L4)");
    expect(container.innerHTML).toContain("<a");
    await act(async () =>
      container.querySelector<HTMLAnchorElement>("a")!.click(),
    );
    expect(onOpenFile).toHaveBeenCalledWith("/Users/me/My Project/main.ts", {
      line: 4,
    });
  });

  it.each([
    "[Source](file://localhost/%2Fhost/share/file.md)",
    "`file://localhost/%2Fhost/share/file.md`",
    "`file://localhost/%5Chost/share/file.md`",
    "`%2F%2Fhost%2Fshare%2Ffile.md`",
    "```12:16:file://localhost/%2Fhost/share/file.md\nexample\n```",
  ])(
    "does not pass a network file URL to the native file opener: %s",
    async (text) => {
      await render(text);
      expect(
        container.querySelector('code[role="link"], .markdown-code-path-link'),
      ).toBeNull();
      for (const link of container.querySelectorAll<HTMLAnchorElement>("a")) {
        await act(async () => link.click());
      }
      expect(onOpenFile).not.toHaveBeenCalled();
    },
  );

  it("opens a code citation at the first displayed source line", async () => {
    await render("```12:16:src/main.ts\nexport const answer = 42;\n```");
    const link = container.querySelector<HTMLButtonElement>(
      ".markdown-code-path-link",
    );
    expect(link).not.toBeNull();
    await act(async () => link!.click());
    expect(onOpenFile).toHaveBeenCalledWith("/repo/src/main.ts", { line: 12 });
  });

  it("preserves external web links and keeps executable URL schemes blocked", async () => {
    await render(
      [
        "[Documentation](https://example.com/docs)",
        "[Script](javascript:alert%281%29)",
        "[Data](data:text/html,bad)",
        "[Hidden](javascript:../src/main.ts)",
      ].join("\n\n"),
    );
    const links = [...container.querySelectorAll<HTMLAnchorElement>("a")];
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "https://example.com/docs",
    ]);
    expect(onOpenFile).not.toHaveBeenCalled();
  });
});
