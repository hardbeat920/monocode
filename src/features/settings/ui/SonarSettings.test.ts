// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { SonarSettings } from "./SonarSettings";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_status")
      return { connected: false, site: "", projectKeys: [] };
    if (command === "sonar_set_config") {
      const config = args as {
        site: string;
        token: string;
        projectKeys: string[];
      };
      if (config.token === "bad-token")
        throw new Error("Sonar denied access. Use Browse permission");
      return {
        connected: Boolean(config.token),
        site: config.site,
        projectKeys: config.projectKeys,
      };
    }
    throw new Error(`Unexpected command ${command}`);
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
it("connects with project keys and keeps the token out of rendered status, then disconnects", async () => {
  await act(async () => root.render(createElement(SonarSettings)));
  await input("Sonar project keys", "org_app, other_app");
  await input("Sonar personal access token", "test-token");
  await submit();
  expect(invoke).toHaveBeenCalledWith("sonar_set_config", {
    site: "https://sonarcloud.io",
    projectKeys: ["org_app", "other_app"],
    token: "test-token",
  });
  expect(container.textContent).toContain("org_app, other_app");
  expect(container.innerHTML).not.toContain("test-token");
  await act(async () => {
    container.querySelector<HTMLButtonElement>("button")!.click();
  });
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_config", {
    site: "",
    projectKeys: [],
    token: "",
  });
  expect(container.textContent).toContain("Connect Sonar");
});
it("keeps the connection form available when Browse access fails", async () => {
  await act(async () => root.render(createElement(SonarSettings)));
  await input("Sonar project keys", "org_app");
  await input("Sonar personal access token", "bad-token");
  await submit();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Browse permission",
  );
  expect(container.querySelector("form")).not.toBeNull();
});
