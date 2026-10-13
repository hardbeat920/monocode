// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { BrowserSettings } from "./BrowserSettings";
import { BrowserIndicator } from "../../sessions/ui/BrowserIndicator";
import { resetBrowserStore, type BrowserState } from "../model/browser";
import { browserState, browserTab } from "../model/browser.fixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));

let container: HTMLDivElement;
let root: Root;
let state: BrowserState;

const granted = {
  session_id: "s1",
  window: "main",
  granted: true,
  turn_active: false,
  granted_since_ms: 1_700_000_000_000,
  tabs: 1,
};

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  state = browserState({ sessions: [granted], tabs: [browserTab()] });
  resetBrowserStore(state);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "browser_audit_list")
      return [
        {
          id: 1,
          ts_ms: 1_700_000_000_000,
          session_id: "s1",
          request_id: "r1",
          global_epoch: "1:1",
          session_epoch: "1:1",
          action: "click",
          url: "https://example.com/",
          params: {},
          outcome: "done",
          duration_ms: 42,
          commands_sent: 3,
        },
        {
          id: 2,
          ts_ms: 1_700_000_000_000,
          session_id: "s1",
          request_id: "r2",
          global_epoch: "1:1",
          session_epoch: "1:1",
          action: "navigate",
          params: {},
          outcome: "blocked",
          reason: "url_denied",
          duration_ms: 0,
          commands_sent: 0,
        },
      ];
    if (command === "browser_audit_clear") return undefined;
    if (command === "browser_mcp_status")
      return [
        {
          provider: "claude",
          supported: true,
          registered: true,
          current: true,
        },
        {
          provider: "codex",
          supported: true,
          registered: true,
          current: false,
        },
        {
          provider: "cursor",
          supported: true,
          registered: false,
          current: false,
        },
        {
          provider: "opencode",
          supported: false,
          registered: false,
          current: false,
          unsupportedReason: "OpenCode 2 is not supported yet.",
        },
      ];
    if (command === "browser_mcp_register")
      return {
        provider: "cursor",
        supported: true,
        registered: true,
        current: true,
      };
    return state;
  });
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  resetBrowserStore();
  vi.unstubAllGlobals();
});

async function renderSettings() {
  await act(async () =>
    root.render(
      createElement(BrowserSettings, {
        sessionName: (id: string) => (id === "s1" ? "Fix login" : id),
        cwd: "/repo",
      }),
    ),
  );
}

function button(text: string): HTMLButtonElement {
  const found = Array.from(
    container.querySelectorAll<HTMLButtonElement>("button"),
  ).find((node) => node.textContent?.trim() === text);
  if (!found) throw new Error(`No button "${text}"`);
  return found;
}

it("switches browser access for all agents", async () => {
  await renderSettings();
  const toggle = container.querySelector<HTMLButtonElement>(
    '[role="switch"][aria-label="Let agents use my browser"]',
  )!;
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  await act(async () => toggle.click());
  expect(invoke).toHaveBeenCalledWith("browser_toggle", { on: false });
});

it("lists threads with their tabs and acts on them", async () => {
  await renderSettings();
  expect(container.textContent).toContain("Fix login");
  expect(container.textContent).toContain("example.com · Idle");

  await act(async () => button("Take back").click());
  expect(invoke).toHaveBeenCalledWith("browser_release_tab", {
    tab: "t_00000001",
  });
  await act(async () => button("Revoke").click());
  expect(invoke).toHaveBeenCalledWith("browser_revoke", { sessionId: "s1" });
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Close Example"]')!
      .click(),
  );
  expect(invoke).toHaveBeenCalledWith("browser_close_tab", {
    tab: "t_00000001",
  });
  await act(async () => button("Close all agent tabs").click());
  expect(invoke).toHaveBeenCalledWith("browser_close_all_tabs", undefined);
});

it("guides the user to turn on remote debugging", async () => {
  state = browserState({
    connection: { state: "needs_toggle", since_ms: 0 },
  });
  resetBrowserStore(state);
  await renderSettings();
  expect(container.textContent).toContain("chrome://inspect/#remote-debugging");
  expect(container.textContent).toContain("edge://inspect");
  await act(async () => button("Try again").click());
  expect(invoke).toHaveBeenCalledWith("browser_connect", undefined);
});

it("offers no button when a policy blocks the browser", async () => {
  resetBrowserStore(
    browserState({ connection: { state: "policy_blocked", since_ms: 0 } }),
  );
  await renderSettings();
  expect(container.textContent).toContain("administrator policy");
  expect(() => button("Try again")).toThrow();
  expect(() => button("Connect")).toThrow();
});

it("shows the activity log and filters it by outcome", async () => {
  vi.useFakeTimers();
  await renderSettings();
  await act(async () => vi.advanceTimersByTimeAsync(200));
  vi.useRealTimers();
  expect(invoke).toHaveBeenCalledWith("browser_audit_list", {
    sessionId: null,
    limit: 50,
  });
  expect(container.textContent).toContain("blocked: url_denied");
  expect(container.textContent).toContain("42 ms");

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Outcome: All outcomes"]')!
      .click(),
  );
  const blocked = Array.from(
    document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ).find((node) => node.textContent === "blocked")!;
  await act(async () => blocked.click());
  expect(container.textContent).toContain("blocked: url_denied");
  expect(container.textContent).not.toContain("42 ms");
});

it("saves clamped limits", async () => {
  await renderSettings();
  const field = container.querySelector<HTMLInputElement>(
    'input[aria-label="Tabs per thread"]',
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(field, "500");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => {
    field.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  });
  expect(invoke).toHaveBeenCalledWith("browser_set_settings", {
    settings: {
      ...state.settings,
      limits: { ...state.settings.limits, tabs_per_session: 64 },
    },
  });
});

it("shows the thread indicator only while the thread has access", async () => {
  await act(async () =>
    root.render(createElement(BrowserIndicator, { sessionId: "s2" })),
  );
  expect(container.querySelector("button")).toBeNull();

  await act(async () =>
    root.render(createElement(BrowserIndicator, { sessionId: "s1" })),
  );
  const indicator = container.querySelector<HTMLButtonElement>(
    '[aria-label="Browser access on"]',
  )!;
  await act(async () => indicator.click());
  const off = Array.from(
    document.querySelectorAll<HTMLButtonElement>("button"),
  ).find((node) => node.textContent === "Turn off")!;
  await act(async () => off.click());
  expect(invoke).toHaveBeenCalledWith("browser_set_session_grant", {
    sessionId: "s1",
    enabled: false,
  });

  await act(async () =>
    resetBrowserStore({
      ...state,
      tabs: [browserTab({ dialog_pending: true })],
    }),
  );
  expect(
    container.querySelector('[aria-label="A tab is waiting on a dialog"]'),
  ).not.toBeNull();
});

it("shows where the browser tools are registered and adds them", async () => {
  await renderSettings();
  const row = container.querySelector('[data-setting-id="browser-mcp"]')!;
  expect(row.textContent).toContain("Claude CodeAdded");
  expect(row.textContent).toContain("Points to an old copy of MonoCode");
  expect(button("Repair")).toBeTruthy();
  expect(row.textContent).toContain("OpenCodeNot supported");
  expect(row.textContent).toContain("OpenCode 2 is not supported yet.");
  // Only Codex (repair) and Cursor (add) offer a button.
  expect(row.querySelectorAll("button")).toHaveLength(2);
  await act(async () => button("Add").click());
  expect(invoke).toHaveBeenCalledWith("browser_mcp_register", {
    provider: "cursor",
    cwd: "/repo",
  });
});
