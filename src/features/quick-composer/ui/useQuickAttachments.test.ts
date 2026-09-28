// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { pickFiles } from "../../../platform/tauri/fs";
import { useQuickAttachments } from "./useQuickAttachments";

const native = vi.hoisted(() => ({ listen: vi.fn(), stop: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: native.listen }),
}));
vi.mock("../../../platform/tauri/fs", () => ({
  pickFiles: vi.fn(),
  basename: (path: string) => path.split("/").pop(),
}));
let api: ReturnType<typeof useQuickAttachments>;
let root: Root;
let container: HTMLDivElement;
let supported = true;
let nativeEvent: (event: {
  payload: { type: string; paths?: string[] };
}) => void;
const onError = vi.fn();
function Harness() {
  api = useQuickAttachments(supported, onError);
  return null;
}
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:preview");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  supported = true;
  vi.mocked(pickFiles).mockResolvedValue(["/tmp/image.png"]);
  vi.mocked(invoke).mockImplementation(async (cmd, args) => {
    if (cmd === "inspect_paths")
      return (args as { paths: string[] }).paths.map((path) => ({
        path,
        name: path.split("/").pop(),
        size: 4,
        isDir: false,
      }));
    if (cmd === "read_file_base64") return "dGVzdA==";
    if (cmd === "write_attachment") return "/tmp/pasted.png";
    if (cmd === "quick_composer_capture") return "/tmp/Screenshot.png";
    throw new Error(`Unexpected command: ${cmd}`);
  });
  native.listen.mockImplementation(async (callback) => {
    nativeEvent = callback;
    return native.stop;
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  act(() => root.render(createElement(Harness)));
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it("chooses files, deduplicates repeat selections, and removes attachments", async () => {
  await act(async () => {
    await api.chooseFiles();
  });
  expect(api.files).toHaveLength(1);
  await act(async () => {
    await api.chooseFiles();
  });
  expect(api.files).toHaveLength(1);
  act(() => api.remove(api.files[0].id));
  expect(api.files).toEqual([]);
});

it("pastes an image into a portable file while retaining its thumbnail", async () => {
  const preventDefault = vi.fn();
  await act(async () => {
    api.onPaste({
      clipboardData: {
        files: [new File(["test"], "pasted.png", { type: "image/png" })],
      },
      preventDefault,
    } as never);
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(preventDefault).toHaveBeenCalled();
  expect(api.files[0]).toMatchObject({
    path: "/tmp/pasted.png",
    previewUrl: "blob:preview",
  });
  act(() => api.clear());
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:preview");
});

it("accepts a native drop and suppresses its duplicate DOM drop", async () => {
  await act(async () => {
    nativeEvent({ payload: { type: "enter" } });
  });
  expect(api.dragging).toBe(true);
  await act(async () => {
    nativeEvent({ payload: { type: "drop", paths: ["/tmp/image.png"] } });
  });
  expect(api.dragging).toBe(false);
  expect(api.files).toHaveLength(1);
  await act(async () => {
    api.onDrop({
      preventDefault: vi.fn(),
      dataTransfer: {
        files: [new File(["test"], "image.png", { type: "image/png" })],
      },
    } as never);
  });
  expect(api.files).toHaveLength(1);
  expect(invoke).not.toHaveBeenCalledWith(
    "write_attachment",
    expect.anything(),
  );
});

it("attaches a screenshot and preserves the draft on cancellation or failure", async () => {
  await act(async () => {
    await api.takeScreenshot();
  });
  expect(api.files[0].path).toBe("/tmp/Screenshot.png");
  vi.mocked(invoke).mockResolvedValueOnce(null);
  await act(async () => {
    await api.takeScreenshot();
  });
  expect(api.files).toHaveLength(1);
  vi.mocked(invoke).mockRejectedValueOnce(new Error("Capture failed"));
  await act(async () => {
    await api.takeScreenshot();
  });
  expect(api.files).toHaveLength(1);
  expect(onError).toHaveBeenLastCalledWith("Capture failed");
  expect(api.loading).toBe(false);
});

it("retains attachments when switching to an unsupported provider", async () => {
  await act(async () => {
    await api.chooseFiles();
  });
  supported = false;
  act(() => root.render(createElement(Harness)));
  await act(async () => {
    await api.chooseFiles();
  });
  expect(pickFiles).toHaveBeenCalledTimes(1);
  expect(api.files).toHaveLength(1);
});

it("shows a recoverable error when persisting a pasted image fails", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(new Error("Disk full"));
  await act(async () => {
    api.onPaste({
      clipboardData: {
        files: [new File(["test"], "pasted.png", { type: "image/png" })],
      },
      preventDefault: vi.fn(),
    } as never);
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(api.files).toEqual([]);
  expect(onError).toHaveBeenLastCalledWith("Disk full");
  expect(api.loading).toBe(false);
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:preview");
});

it("releases discarded screenshots without releasing the remaining draft", async () => {
  await act(async () => {
    await api.takeScreenshot();
  });
  const id = api.files[0].id;
  act(() => api.remove(id));
  expect(invoke).toHaveBeenCalledWith("quick_composer_release_capture", {
    paths: ["/tmp/Screenshot.png"],
  });
  expect(api.files).toHaveLength(0);
});

it("releases a capture when image inspection fails", async () => {
  const impl = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "inspect_paths") throw new Error("Cannot decode capture");
    if (command === "quick_composer_release_capture") return undefined;
    return impl(command, args);
  });
  await act(async () => {
    await api.takeScreenshot();
  });
  expect(api.files).toHaveLength(0);
  expect(invoke).toHaveBeenCalledWith("quick_composer_release_capture", {
    paths: ["/tmp/Screenshot.png"],
  });
});
