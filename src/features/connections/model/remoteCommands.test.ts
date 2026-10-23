import { beforeEach, expect, it, vi } from "vitest";

const { remoteRequest, remoteMachineFor, invokeLocal } = vi.hoisted(() => ({
  remoteRequest: vi.fn(),
  invokeLocal: vi.fn(),
  remoteMachineFor: vi.fn(async (environmentId: string) =>
    environmentId === "env"
      ? { id: "machine", name: "Home", endpoint: "", environmentId }
      : undefined,
  ),
}));
vi.mock("./connections", () => ({ remoteRequest, remoteMachineFor }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeLocal }));

import { runRemoteCommand } from "./remoteCommands";
import { parseRemotePath, remotePath } from "./remoteProjects";
import { gitCommit, gitLocateFiles, gitStagedContext, listDir, readBinaryFile, readTextFile, statFiles, writeTextFile } from "../../../platform/tauri/fs";
import { checkCowRemoval, cowCapability, listCowWorkspaces } from "../../source-control/model/cow";
import { listWorktrees } from "../../source-control/model/worktrees";

beforeEach(() => {
  remoteRequest.mockReset();
  invokeLocal.mockReset();
});

it("keeps local writes local when their content mentions a remote path", async () => {
  await writeTextFile("/home/me/note.txt", "remote://env/home/me/repo");
  expect(invokeLocal).toHaveBeenCalledWith("write_text_file", {
    path: "/home/me/note.txt",
    content: "remote://env/home/me/repo",
  });
  expect(remoteRequest).not.toHaveBeenCalled();
});

it("runs the same file command on the machine with host paths", async () => {
  remoteRequest.mockResolvedValue([
    { name: "src", path: "/home/me/repo/src", isDir: true, ignored: false },
  ]);
  expect(await listDir("remote://env/home/me/repo")).toEqual([
    {
      name: "src",
      path: "remote://env/home/me/repo/src",
      isDir: true,
      ignored: false,
    },
  ]);
  expect(remoteRequest).toHaveBeenCalledWith("machine", "workspace.run", {
    command: "list_dir",
    args: { path: "/home/me/repo" },
  });
});

it("relays CoW cleanup preflight to the owning host", async () => {
  remoteRequest.mockResolvedValue(undefined);
  await checkCowRemoval("remote://env/home/me/repo", "owned-copy", true);
  expect(remoteRequest).toHaveBeenCalledExactlyOnceWith("machine", "workspace.run", {
    command: "cow_check_remove",
    args: { cwd: "/home/me/repo", cowId: "owned-copy", force: true },
  });
  expect(invokeLocal).not.toHaveBeenCalled();
});

it("keeps worktree discovery usable when an older host has no CoW commands", async () => {
  remoteRequest.mockRejectedValueOnce(new Error("Host rejected request: Unsupported workspace command"));
  expect(await listCowWorkspaces("remote://env/home/me/repo")).toEqual([]);
  remoteRequest.mockResolvedValueOnce({
    defaultRoot: "/home/me/repo-worktrees",
    worktrees: [{ path: "/home/me/repo-worktrees/feature", branch: "feature" }],
  });
  expect(await listWorktrees("remote://env/home/me/repo")).toMatchObject({
    defaultRoot: "remote://env/home/me/repo-worktrees",
    worktrees: [{ path: "remote://env/home/me/repo-worktrees/feature", branch: "feature" }],
  });
  expect(invokeLocal).not.toHaveBeenCalled();
});

it("explains unavailable CoW on older hosts and refuses cleanup mutations", async () => {
  remoteRequest.mockRejectedValue(new Error("Unsupported workspace command"));
  expect(await cowCapability("remote://env/home/me/repo")).toMatchObject({
    supported: false,
    reason: expect.stringContaining("Update MonoCode Host"),
  });
  await expect(checkCowRemoval("remote://env/home/me/repo", "owned-copy", true))
    .rejects.toThrow("Update MonoCode Host");
});

it.each(["Host disconnected", "Host rejected request: Isolation root was replaced"])(
  "preserves real CoW discovery failures: %s",
  async (message) => {
    remoteRequest.mockRejectedValue(new Error(message));
    await expect(listCowWorkspaces("remote://env/home/me/repo")).rejects.toThrow(message);
    await expect(cowCapability("remote://env/home/me/repo")).rejects.toThrow(message);
  },
);

it("maps path results back and leaves file contents alone", async () => {
  remoteRequest.mockResolvedValueOnce("remote://not-a-path");
  expect(await readTextFile("remote://env/home/me/a.txt")).toBe(
    "remote://not-a-path",
  );
  remoteRequest.mockResolvedValueOnce([
    { path: "/home/me/a.txt", mtimeMs: 1 },
  ]);
  expect(await statFiles(["remote://env/home/me/a.txt"])).toEqual([
    { path: "remote://env/home/me/a.txt", mtimeMs: 1 },
  ]);
  remoteRequest.mockResolvedValueOnce("/home/me/b.txt");
  expect(
    await runRemoteCommand("rename_path", {
      path: "remote://env/home/me/a.txt",
      name: "b.txt",
    }),
  ).toBe("remote://env/home/me/b.txt");
  remoteRequest.mockResolvedValueOnce("C:/work/x.ts");
  expect(
    await runRemoteCommand("create_path", {
      parent: "remote://env/C:/work",
      name: "x.ts",
      isDir: false,
    }),
  ).toBe("remote://env/C:/work/x.ts");
  expect(remoteRequest).toHaveBeenLastCalledWith("machine", "workspace.run", {
    command: "create_path",
    args: { parent: "C:/work", name: "x.ts", isDir: false },
  });
});

it("decodes remote binary reads", async () => {
  remoteRequest.mockResolvedValueOnce("AAEC/w==");
  expect(await readBinaryFile("remote://env/home/me/image.png")).toEqual(
    new Uint8Array([0, 1, 2, 255]),
  );
});

it("preserves Windows drive and UNC paths", () => {
  expect(parseRemotePath(remotePath("env", "C:\\work\\repo"))?.hostPath).toBe(
    "C:/work/repo",
  );
  expect(parseRemotePath(remotePath("env", "\\\\server\\share\\repo"))?.hostPath).toBe(
    "//server/share/repo",
  );
});

it("adds remote paths to Git index entries", async () => {
  remoteRequest.mockResolvedValueOnce({
    branch: "main",
    files: [{ path: "src/app.ts", relative: "src/app.ts", status: "modified" }],
  });
  const index = await runRemoteCommand("git_diff_index", {
    cwd: "remote://env/home/me/repo",
  }) as { files: { path: string }[] };
  expect(index.files[0].path).toBe("remote://env/home/me/repo/src/app.ts");
});

it("locates remote checkout roots without converting relative filenames", async () => {
  remoteRequest.mockResolvedValueOnce([
    { root: "/home/me/a", relative: "same.txt" }, null,
    { root: "/home/me/b", relative: "same.txt" },
  ]);
  const paths = ["remote://env/home/me/a/same.txt", "remote://env/home/me/outside.txt", "remote://env/home/me/b/same.txt"];
  expect(await gitLocateFiles(paths)).toEqual([
    { root: "remote://env/home/me/a", relative: "same.txt" }, null,
    { root: "remote://env/home/me/b", relative: "same.txt" },
  ]);
  expect(remoteRequest).toHaveBeenCalledWith("machine", "workspace.run", {
    command: "git_locate_files", args: { paths: paths.map((path) => path.replace("remote://env", "")) },
  });
});

it("passes selected files through remote commit and context calls as repo-relative paths", async () => {
  await gitCommit("remote://env/home/me/a", "Selected", false, ["same.txt", "line\nbreak"]);
  expect(remoteRequest).toHaveBeenLastCalledWith("machine", "workspace.run", {
    command: "git_commit", args: { cwd: "/home/me/a", message: "Selected", amend: false, paths: ["same.txt", "line\nbreak"] },
  });
  await gitStagedContext("remote://env/home/me/b", ["same.txt"]);
  expect(remoteRequest).toHaveBeenLastCalledWith("machine", "workspace.run", {
    command: "git_staged_context", args: { cwd: "/home/me/b", paths: ["same.txt"] },
  });
  expect(invokeLocal).not.toHaveBeenCalled();
});

it("routes project search through the host and maps match paths", async () => {
  remoteRequest.mockResolvedValueOnce({
    matches: [{ path: "/home/me/repo/src/app.ts", relative: "src/app.ts", line: 4 }],
    truncated: false,
  });
  expect(await runRemoteCommand("search_project", {
    options: { cwd: "remote://env/home/me/repo", query: "hello" },
  })).toMatchObject({
    matches: [{ path: "remote://env/home/me/repo/src/app.ts", line: 4 }],
  });
  expect(remoteRequest).toHaveBeenCalledWith("machine", "workspace.run", {
    command: "search_project",
    args: { options: { cwd: "/home/me/repo", query: "hello" } },
  });
});

it("refuses what the host cannot do and explains outdated hosts", async () => {
  await expect(
    runRemoteCommand("reveal_path", { path: "remote://env/home/me/a" }),
  ).rejects.toThrow("isn’t available for projects on another machine");
  await expect(
    runRemoteCommand("copy_path", {
      from: "/Users/me/local.txt",
      destParent: "remote://env/home/me",
    }),
  ).rejects.toThrow("within one machine");
  await expect(
    runRemoteCommand("move_path", {
      from: "remote://env/home/me/a",
      destParent: "remote://other/home/me",
    }),
  ).rejects.toThrow("within one machine");
  await expect(
    runRemoteCommand("list_dir", { path: "remote://gone/home/me" }),
  ).rejects.toThrow("isn’t connected");
  remoteRequest.mockRejectedValueOnce("Unsupported remote operation");
  await expect(
    runRemoteCommand("list_dir", { path: "remote://env/home/me" }),
  ).rejects.toThrow("Update MonoCode Host");
  expect(remoteRequest).toHaveBeenCalledTimes(1);
});
