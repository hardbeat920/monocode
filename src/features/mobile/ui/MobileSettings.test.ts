// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { MobileSettings } from "./MobileSettings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("qrcode", () => ({
  default: { toDataURL: vi.fn(async () => "data:image/png;base64,qr") },
}));

let container: HTMLDivElement;
let root: Root;
let enabled: boolean;
let devices: { id: string; name: string; fingerprint: string }[];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  enabled = false;
  devices = [];
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "mobile_status")
      return {
        enabled,
        listening: enabled,
        name: "Desktop",
        fingerprint: null,
        openRouter: false,
        remoteAddress: "",
        routerStatus: "Off",
        addresses: [],
        devices: [...devices],
      };
    if (command === "mobile_configure") {
      enabled = (args as { enabled: boolean }).enabled;
      return;
    }
    if (command === "mobile_invite")
      return { url: "monocode://pair?i=x", expires: Date.now() / 1000 + 900 };
    if (command === "mobile_revoke") {
      devices = devices.filter(
        (device) => device.id !== (args as { deviceId: string }).deviceId,
      );
      return;
    }
    if (command === "mobile_cancel_invite") return;
    throw new Error(`Unexpected command ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const button = (name: string) =>
  [...document.body.querySelectorAll("button")].find(
    (button) => button.textContent?.trim() === name,
  )!;
const settle = () => act(async () => void (await vi.advanceTimersByTimeAsync(0)));

it("turns on mobile access, shows the code, and closes it once a phone pairs", async () => {
  await act(async () => root.render(createElement(MobileSettings)));
  await settle();
  expect(container.textContent).toContain("Pair your iPhone or Android phone");

  await act(async () => button("Pair phone").click());
  await settle();
  expect(invoke).toHaveBeenCalledWith(
    "mobile_configure",
    expect.objectContaining({ enabled: true }),
  );
  const dialog = document.body.querySelector('[role="dialog"]');
  expect(dialog?.querySelector("img")?.getAttribute("src")).toContain("qr");
  expect(dialog?.textContent).toContain("Expires in 15:00");

  devices = [{ id: "p1", name: "Nick’s iPhone", fingerprint: "f" }];
  await act(async () => void (await vi.advanceTimersByTimeAsync(2000)));
  expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  expect(container.textContent).toContain("Nick’s iPhone is paired.");
});

it("asks before removing a paired phone", async () => {
  enabled = true;
  devices = [{ id: "p1", name: "Pixel", fingerprint: "f" }];
  await act(async () => root.render(createElement(MobileSettings)));
  await settle();

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Remove Pixel"]')!
      .click(),
  );
  expect(invoke).not.toHaveBeenCalledWith("mobile_revoke", expect.anything());
  await act(async () => button("Remove phone").click());
  await settle();
  expect(invoke).toHaveBeenCalledWith("mobile_revoke", { deviceId: "p1" });
  expect(container.textContent).toContain("Pixel was removed.");
});
