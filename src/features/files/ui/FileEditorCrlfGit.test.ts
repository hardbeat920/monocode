// @vitest-environment happy-dom
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { EditorView } from "@codemirror/view";
import type * as TauriCore from "@tauri-apps/api/core";
import { Storage } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { diffNavigablePositions, stageChunkAt } from "../editor/editorGit";
import { FileEditor } from "./FileEditor";

const bridge = vi.hoisted(() => ({
  invoke: (_command: string, _args?: Record<string, unknown>): unknown =>
    undefined,
}));
// Keep the editor and Git behavior real; replace only native transport.
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof TauriCore>()),
  invoke: async (command: string, args?: Record<string, unknown>) =>
    bridge.invoke(command, args),
}));

describe("CRLF editor Git boundaries", () => {
  let directory: string;
  let root: Root;
  let container: HTMLDivElement;
  let gitEnvironment: NodeJS.ProcessEnv;
  const filename = "notes.txt";
  const git = (args: string[], input?: string) =>
    execFileSync("git", ["-C", directory, ...args], {
      encoding: "utf8",
      input,
      env: gitEnvironment,
      stdio: ["pipe", "pipe", "pipe"],
    });
  const write = (content: string) =>
    writeFileSync(join(directory, filename), content);
  const disk = () => readFileSync(join(directory, filename), "utf8");
  const index = () => git(["show", `:${filename}`]);
  function baseline(content: string, autocrlf = "false") {
    git(["init", "-q"]);
    git(["config", "user.name", "CRLF Test"]);
    git(["config", "user.email", "crlf@example.invalid"]);
    git(["config", "commit.gpgsign", "false"]);
    git(["config", "core.autocrlf", autocrlf]);
    git(["config", "core.safecrlf", "false"]);
    write(content);
    git(["add", "--", filename]);
    git(["commit", "-qm", "Initial content"]);
  }
  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), "monocode-crlf-git-"));
    gitEnvironment = {
      ...process.env,
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_CONFIG_GLOBAL: join(directory, "empty-config"),
    };
    writeFileSync(join(directory, "empty-config"), "");
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("localStorage", new Storage());
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    bridge.invoke = (command, args) => {
      if (command === "read_text_file") return disk();
      if (command === "stat_files") return [];
      if (command === "git_diff_files") {
        const status = git(["status", "--porcelain", "--", filename]);
        return {
          files: status
            ? [
                {
                  relative: filename,
                  staged: status[0] !== " " && status[0] !== "?",
                  unstaged: status[1] !== " ",
                },
              ]
            : [],
        };
      }
      if (command === "git_file_diff") {
        const staged = !!args?.staged;
        const exists =
          git(
            staged
              ? ["ls-tree", "--name-only", "HEAD", "--", filename]
              : ["ls-files", "--", filename],
          ).trim() !== "";
        return {
          original: exists
            ? git(["show", `${staged ? "HEAD:" : ":"}${filename}`])
            : "",
          current: staged ? index() : disk(),
          binary: false,
          tooLarge: false,
        };
      }
      if (command === "git_stage_contents") {
        const hash = git(
          ["hash-object", "-w", "--path", filename, "--stdin"],
          args?.contents as string,
        ).trim();
        git(["update-index", "--add", "--cacheinfo", "100644", hash, filename]);
        return;
      }
      throw new Error(`Unexpected native command: ${command}`);
    };
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    rmSync(directory, { recursive: true, force: true });
  });
  async function render() {
    const cwd = directory.replaceAll("\\", "/");
    await act(async () =>
      root.render(
        createElement(FileEditor, {
          path: `${cwd}/${filename}`,
          cwd,
          active: true,
          showDiff: true,
          onDirtyChange: () => {},
        }),
      ),
    );
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelector(".cm-editor")).not.toBeNull(),
      ),
    );
    return EditorView.findFromDOM(
      container.querySelector<HTMLElement>(".cm-editor")!,
    )!;
  }

  it("shows staged hunks under autocrlf without rewriting staged siblings", async () => {
    const original = "old first\n" + "context\n".repeat(16) + "old last\n";
    const changed = original
      .replace("old first", "new first")
      .replace("old last", "new last");
    baseline(original, "true");
    write(changed.replaceAll("\n", "\r\n"));
    git(["add", "--", filename]);
    const view = await render();
    await act(async () =>
      vi.waitFor(() => expect(diffNavigablePositions(view)).toHaveLength(2)),
    );
    expect(view.state.doc.toString()).toBe(changed);
    await act(async () => expect(await stageChunkAt(view, 0)).toBe(false));
    expect(index()).toBe(changed);
    expect(disk()).toBe(changed.replaceAll("\n", "\r\n"));
  });

  it("does not confuse a real CRLF-only change with a clean working tree", async () => {
    baseline("old\nline\n");
    write("new\nline\n");
    git(["add", "--", filename]);
    write("new\r\nline\r\n");
    const view = await render();
    await act(async () =>
      vi.waitFor(() =>
        expect(container.querySelector('[role="status"]')).not.toBeNull(),
      ),
    );
    expect(diffNavigablePositions(view)).toEqual([]);
    expect(git(["diff", "--name-only", "--", filename]).trim()).toBe(filename);
    expect(index()).toBe("new\nline\n");
  });

  it("keeps the index EOL when staging only one CRLF editor hunk", async () => {
    const original = "old first\n" + "context\n".repeat(16) + "old last\n";
    const changed = original
      .replace("old first", "new first")
      .replace("old last", "new last");
    baseline(original);
    write(changed.replaceAll("\n", "\r\n"));
    const view = await render();
    await act(async () =>
      vi.waitFor(() => expect(diffNavigablePositions(view)).toHaveLength(2)),
    );
    await act(async () => expect(await stageChunkAt(view, 0)).toBe(true));
    expect(index()).toBe(original.replace("old first", "new first"));
    expect(disk()).toBe(changed.replaceAll("\n", "\r\n"));
  });

  it("keeps CRLF when staging a file without an index baseline", async () => {
    baseline("");
    git(["rm", "--cached", "--", filename]);
    git(["commit", "-qm", "Remove file from index"]);
    write("first\r\nsecond\r\n");
    const view = await render();
    await act(async () =>
      vi.waitFor(() => expect(diffNavigablePositions(view)).toHaveLength(1)),
    );
    await act(async () => expect(await stageChunkAt(view, 0)).toBe(true));
    expect(index()).toBe("first\r\nsecond\r\n");
    expect(disk()).toBe(index());
  });
});
