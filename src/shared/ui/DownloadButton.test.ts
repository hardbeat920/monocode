// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { downloadFile } from "../../platform/tauri/download";
import { DownloadButton } from "./DownloadButton";

vi.mock("../../platform/tauri/download", () => ({ downloadFile: vi.fn() }));

let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(downloadFile).mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function render(path = "/report.pdf") {
  act(() =>
    root.render(
      createElement(DownloadButton, { name: "report.pdf", source: { path } }),
    ),
  );
}
function button() {
  return container.querySelector<HTMLButtonElement>(
    '[aria-label="Download report.pdf"]',
  )!;
}

it("keeps the button icon-only and guards clicks until the download settles", async () => {
  let finish!: () => void;
  vi.mocked(downloadFile).mockReturnValue(
    new Promise<void>((resolve) => {
      finish = resolve;
    }),
  );
  render();
  expect(button().textContent).toBe("");
  act(() => {
    button().click();
    button().click();
  });
  expect(downloadFile).toHaveBeenCalledTimes(1);
  expect(button().disabled).toBe(true);
  expect(button().getAttribute("aria-busy")).toBe("true");
  await act(async () => finish());
  expect(button().disabled).toBe(false);
  expect(button().getAttribute("aria-busy")).toBeNull();
});

it("shows a dismissible error and permits a successful retry", async () => {
  vi.mocked(downloadFile).mockRejectedValueOnce(new Error("Disk full"));
  render();
  await act(async () => button().click());
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Disk full",
  );
  act(() =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Dismiss download error"]',
      )!
      .click(),
  );
  expect(container.querySelector('[role="alert"]')).toBeNull();
  await act(async () => button().click());
  expect(downloadFile).toHaveBeenCalledTimes(2);
  expect(container.querySelector('[role="alert"]')).toBeNull();
});
