// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AsanaSettings } from "./AsanaSettings";
import {
  loadAsanaProjectLinks,
  loadHiddenAsanaProjectIds,
} from "../../inbox/model/asana";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.mocked(openUrl).mockReset();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "asana_status")
      return { connected: false, name: "", email: "" };
    if (command === "asana_set_token") {
      const { token } = args as { token: string };
      return token
        ? { connected: true, name: "Ada Lovelace", email: "ada@example.com" }
        : { connected: false, name: "", email: "" };
    }
    if (command === "asana_list_projects")
      return [{ id: "1200000000000001", key: "Acme", name: "Launch" }];
    throw new Error(`Unexpected command: ${command}`);
  });
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

async function input(label: string, value: string) {
  const field = container.querySelector<HTMLInputElement>(
    `input[aria-label="${label}"]`,
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function submit() {
  await act(async () => {
    container
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
}

it("connects Asana with a token, synchronizes project filters, and disconnects", async () => {
  await act(async () => root.render(createElement(AsanaSettings)));
  expect(container.querySelectorAll("input")).toHaveLength(1);
  await input("Asana personal access token", " secret ");
  await submit();
  expect(invoke).toHaveBeenCalledWith("asana_set_token", { token: "secret" });
  expect(container.textContent).toContain("Ada Lovelace");
  expect(container.textContent).toContain("ada@example.com");
  expect(container.textContent).toContain("Acme");
  expect(container.querySelector('input[type="password"]')).toBeNull();
  const project = container.querySelector<HTMLInputElement>(
    'input[type="checkbox"]',
  )!;
  expect(project.checked).toBe(true);
  await act(async () => project.click());
  expect(loadHiddenAsanaProjectIds()).toEqual(["1200000000000001"]);
  expect(project.checked).toBe(false);
  const disconnect = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Disconnect",
  )!;
  await act(async () => disconnect.click());
  expect(invoke).toHaveBeenCalledWith("asana_set_token", { token: "" });
  expect(
    container.querySelector<HTMLInputElement>('input[type="password"]')!.value,
  ).toBe("");
});

it("links an Asana project to a MonoCode project", async () => {
  await act(async () =>
    root.render(
      createElement(AsanaSettings, {
        localProjects: ["/work/app", "/work/site/"],
      }),
    ),
  );
  await input("Asana personal access token", "secret");
  await submit();
  const select = container.querySelector<HTMLSelectElement>(
    'select[aria-label="MonoCode project for Launch"]',
  )!;
  expect([...select.options].map((option) => option.textContent)).toEqual([
    "Not linked",
    "app",
    "site",
  ]);
  await act(async () => {
    select.value = "/work/site";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(loadAsanaProjectLinks()).toEqual({ "1200000000000001": "/work/site" });
  expect(select.value).toBe("/work/site");
  await act(async () => {
    select.value = "";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(loadAsanaProjectLinks()).toEqual({});
});

it("links to the Asana developer console for a new token", async () => {
  await act(async () => root.render(createElement(AsanaSettings)));
  const link = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Create access token",
  )!;
  await act(async () => link.click());
  expect(openUrl).toHaveBeenCalledWith("https://app.asana.com/0/my-apps");
});

it("shows authentication errors without claiming a successful connection", async () => {
  await act(async () => root.render(createElement(AsanaSettings)));
  await input("Asana personal access token", "bad-token");
  vi.mocked(invoke).mockRejectedValueOnce(
    new Error("Asana personal access token is invalid"),
  );
  await submit();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "invalid",
  );
  expect(container.querySelector("form")).not.toBeNull();
  expect(container.textContent).not.toContain("Projects");
});
