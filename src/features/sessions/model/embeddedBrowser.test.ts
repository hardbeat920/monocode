import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
});

it("closes only after pending creation finishes", async () => {
  const { browserRequest } = await import("./embeddedBrowser");
  let finish!: () => void;
  invoke.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  invoke.mockResolvedValueOnce(null);
  const opening = browserRequest({
    action: "open",
    url: "https://example.com",
    bounds: { x: 0, y: 0, width: 600, height: 400 },
  });
  const closing = browserRequest({ action: "close" });
  await Promise.resolve();
  expect(invoke).toHaveBeenCalledTimes(1);
  finish();
  await Promise.all([opening, closing]);
  expect(invoke.mock.calls[1][1]).toEqual({ request: { action: "close" } });
});

it("allows retry after a failed native operation", async () => {
  const { browserRequest } = await import("./embeddedBrowser");
  invoke.mockRejectedValueOnce(new Error("creation failed"));
  await expect(browserRequest({ action: "reload" })).rejects.toThrow(
    "creation failed",
  );
  invoke.mockResolvedValueOnce(null);
  await expect(browserRequest({ action: "close" })).resolves.toBeNull();
});
