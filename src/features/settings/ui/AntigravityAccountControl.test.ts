// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AntigravityAccountControl } from "./AntigravityAccountControl";

const mock = vi.hoisted(() => ({ status: vi.fn(), login: vi.fn(), logout: vi.fn(), refresh: vi.fn(), cancel: vi.fn(), unlisten: vi.fn(), listener: undefined as undefined | (() => void) }));
vi.mock("../../../integrations/harness/core/auth", () => ({ loginHarness: mock.login }));
vi.mock("../../../integrations/harness/providers/antigravity/antigravityNative", () => ({ nativeAntigravityAccount: mock.status, logoutNativeAntigravity: mock.logout, refreshNativeAntigravityCatalog: mock.refresh, cancelNativeAntigravityLogin: mock.cancel }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ listen: async (_name: string, callback: () => void) => { mock.listener = callback; return mock.unlisten; } }) }));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  mock.status.mockResolvedValue({ backendAvailable: true, authenticated: false, loginPending: false });
  mock.login.mockResolvedValue(undefined); mock.logout.mockResolvedValue(undefined); mock.refresh.mockResolvedValue(undefined); mock.cancel.mockResolvedValue(undefined);
  container = document.createElement("div"); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); });
const render = async () => { await act(async () => root.render(createElement(AntigravityAccountControl, { cwd: "C:/test" }))); };
function button(text: string): HTMLButtonElement { return [...container.querySelectorAll("button")].find((button) => button.textContent?.includes(text))!; }

it("checks status without opening OAuth and connects only after a click", async () => {
  await render();
  expect(container.textContent).toContain("Not signed in"); expect(mock.login).not.toHaveBeenCalled();
  await act(async () => button("Connect Google").click());
  expect(mock.login).toHaveBeenCalledWith("antigravity", undefined, "C:/test"); expect(mock.status).toHaveBeenCalledTimes(2);
});

it("shows account errors while keeping the backend connection action available", async () => {
  mock.status.mockResolvedValue({ backendAvailable: true, authenticated: false, loginPending: false, authError: "Could not read Antigravity data" });
  await render();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain("Could not read");
  expect(button("Connect Google").disabled).toBe(false);
});

it("refreshes models and disconnects the connected account", async () => {
  mock.status.mockResolvedValue({ backendAvailable: true, authenticated: true, email: "one@example.test", loginPending: false });
  await render(); expect(container.textContent).toContain("one@example.test");
  await act(async () => button("Refresh models").click()); expect(mock.refresh).toHaveBeenCalledTimes(1);
  mock.status.mockResolvedValue({ backendAvailable: true, authenticated: false, loginPending: false });
  await act(async () => button("Disconnect Google").click()); expect(mock.logout).toHaveBeenCalledTimes(1);
  expect(container.textContent).toContain("Not signed in");
});

it("offers cancellation while explicit sign-in is pending", async () => {
  let finish!: () => void; mock.login.mockReturnValue(new Promise<void>((resolve) => { finish = resolve; }));
  await render(); await act(async () => button("Connect Google").click());
  await act(async () => button("Cancel sign-in").click()); expect(mock.cancel).toHaveBeenCalledTimes(1);
  await act(async () => finish());
});
