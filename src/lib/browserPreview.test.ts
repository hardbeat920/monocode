import { describe, expect, it, vi } from "vitest";
import {
  localPreviewScanner,
  localPreviewUrls,
  claimLocalPreview,
  previewUrl,
} from "./browserPreview";
import {
  isFilesystemTab,
  newFileTab,
  newTab,
  openBrowserTab,
  openEditorTab,
} from "./layout";
import {
  collectWorkspaceSnapshot,
  hydrateWorkspaceSnapshot,
} from "./workspaceSnapshot";
import { newSession } from "./session";

describe("web preview addresses", () => {
  it("accepts HTTP(S) and rejects native schemes, credentials and app hosts", () => {
    expect(previewUrl(" https://example.com/docs ")).toBe(
      "https://example.com/docs",
    );
    for (const url of [
      "file:///etc/passwd",
      "javascript:alert(1)",
      "data:text/html,hi",
      "tauri://localhost",
      "http://asset.localhost/a",
      "https://user:secret@example.com",
      "//example.com",
      "not a URL",
    ])
      expect(previewUrl(url), url).toBeUndefined();
  });

  it("only auto-opens loopback servers and normalizes wildcard bind addresses", () => {
    expect(
      localPreviewUrls(
        "\x1b[32mLocal: http://localhost:5173/\x1b[0m\nNetwork: http://192.168.1.2:5173\nhttps://example.com:444\nhttp://localhost.evil.test:3000\nhttp://0.0.0.0:8080\nhttp://[::1]:3000/app",
      ),
    ).toEqual([
      "http://localhost:5173/",
      "http://127.0.0.1:8080/",
      "http://[::1]:3000/app",
    ]);
    expect(localPreviewUrls("http://localhost http://127.0.0.1")).toEqual([]);
  });

  it("waits for a split terminal URL to finish instead of opening a partial port", () => {
    const found = vi.fn();
    const scan = localPreviewScanner(found);
    scan("Local: http://local");
    scan("host:3");
    expect(found).not.toHaveBeenCalled();
    scan("000/\n");
    expect(found).toHaveBeenCalledExactlyOnceWith("http://localhost:3000/");
    scan("http://127.0.0.1:8080");
    scan("", true);
    expect(found).toHaveBeenLastCalledWith("http://127.0.0.1:8080/");
  });
});

describe("preview tabs", () => {
  it("deduplicates a server after closing the preview without suppressing another workspace", () => {
    const seen = new Set<string>();
    expect(claimLocalPreview("http://localhost:5173/", seen)).toBe(
      "http://localhost:5173/",
    );
    expect(
      claimLocalPreview("http://localhost:5173/again", seen),
    ).toBeUndefined();
    expect(claimLocalPreview("https://example.com:443", seen)).toBeUndefined();
    expect(claimLocalPreview("http://localhost:5173/", new Set())).toBe(
      "http://localhost:5173/",
    );
  });
  it("opens in the existing right pane, stays out of filesystem actions, and reuses its tab", () => {
    let tab = openEditorTab(newTab("s"), newFileTab("/repo/app.ts", "/repo"));
    const paneId = tab.editorPanes[0].id;
    tab = openBrowserTab(tab, "/repo", "http://localhost:3000/");
    expect(tab.editorPanes).toHaveLength(1);
    const browser = tab.editorPanes[0].files[1];
    expect(isFilesystemTab(browser)).toBe(false);
    expect(tab.focusedId).toBe(paneId);
    const next = openBrowserTab(tab, "/repo", "http://localhost:4000/");
    expect(next.editorPanes[0].files).toHaveLength(2);
    expect(next.editorPanes[0].files[1]).toMatchObject({
      id: browser.id,
      browser: { url: "http://localhost:4000/" },
    });
    expect(
      openBrowserTab(next, "/repo").editorPanes[0].files[1].browser?.url,
    ).toBe("http://localhost:4000/");
  });

  it("does not restore a website or mistake its tab for a local file", () => {
    const session = newSession("codex", "/repo");
    session.blocks = [{ id: "u", role: "user", text: "Build a page" }];
    const tab = openBrowserTab(
      newTab(session.id),
      "/repo",
      "https://example.com/private",
    );
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [session],
      tab.id,
      "/repo",
    );
    const restored = hydrateWorkspaceSnapshot(
      snapshot,
      new Map([[session.id, session]]),
    );
    expect(restored?.tabs[0].editorPanes[0].files[0]).toMatchObject({
      path: "Browser",
      browser: { url: "" },
    });
    expect(JSON.stringify(snapshot)).not.toContain("example.com");
  });
});
