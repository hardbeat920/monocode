// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { JiraSettings } from "./JiraSettings";
import { loadHiddenJiraProjectIds } from "../../inbox/model/jira";
import { saveMaskEmails } from "../model/displayPrefs";

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
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "jira_status")
      return { connected: false, site: "", email: "" };
    if (command === "jira_set_config") {
      const config = args as { site: string; email: string; token: string };
      return {
        connected: Boolean(config.token),
        site: config.site,
        email: config.email,
      };
    }
    if (command === "jira_list_projects")
      return [{ id: "10000", key: "ENG", name: "Engineering" }];
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

it("connects Jira, synchronizes project filters, and disconnects", async () => {
  saveMaskEmails(true);
  await act(async () => root.render(createElement(JiraSettings)));
  await input("Jira site", "acme.atlassian.net");
  await input("Atlassian email", "ada@example.com");
  await input("Jira API token", "secret");
  await submit();
  expect(invoke).toHaveBeenCalledWith("jira_set_config", {
    site: "acme.atlassian.net",
    email: "ada@example.com",
    token: "secret",
  });
  expect(container.textContent).toContain("ada@example.com");
  const email = container.querySelector<HTMLButtonElement>(
    '[aria-label="Reveal email"]',
  )!;
  expect(email.querySelector("span")?.className).toContain("blur-[5px]");
  await act(async () => email.click());
  expect(email.getAttribute("aria-label")).toBe("Hide email");
  await act(async () => email.click());
  expect(email.getAttribute("aria-label")).toBe("Reveal email");
  expect(container.querySelector('input[type="password"]')).toBeNull();
  const project = container.querySelector<HTMLInputElement>(
    'input[type="checkbox"]',
  )!;
  expect(project.checked).toBe(true);
  await act(async () => project.click());
  expect(loadHiddenJiraProjectIds()).toEqual(["10000"]);
  expect(project.checked).toBe(false);
  const disconnect = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Disconnect",
  )!;
  await act(async () => disconnect.click());
  expect(invoke).toHaveBeenCalledWith("jira_set_config", {
    site: "",
    email: "",
    token: "",
  });
  expect(
    container.querySelector<HTMLInputElement>('input[type="password"]')!.value,
  ).toBe("");
});

it("shows authentication errors without claiming a successful connection", async () => {
  await act(async () => root.render(createElement(JiraSettings)));
  await input("Jira site", "acme");
  await input("Atlassian email", "ada@example.com");
  await input("Jira API token", "bad-token");
  vi.mocked(invoke).mockRejectedValueOnce(
    new Error("Jira email or API token is invalid"),
  );
  await submit();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "invalid",
  );
  expect(container.querySelector("form")).not.toBeNull();
  expect(container.textContent).not.toContain("Projects");
});

it("connects Jira Data Center with a personal access token and no email", async () => {
  await act(async () => root.render(createElement(JiraSettings)));
  await input("Jira site", "jira.example.com");
  await input("Jira API token", "pat");
  await submit();
  expect(invoke).toHaveBeenCalledWith("jira_set_config", {
    site: "jira.example.com",
    email: "",
    token: "pat",
  });
  expect(container.textContent).toContain("Personal access token");
  expect(container.textContent).toContain("Projects");
});

it("filters projects and selects or unselects every matching project", async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "jira_status")
      return { connected: true, site: "https://jira.example.com", email: "" };
    if (command === "jira_list_projects")
      return [
        { id: "1", key: "ENG", name: "Engineering" },
        { id: "2", key: "OPS", name: "Operations" },
        { id: "3", key: "ENGX", name: "Engineering Extras" },
      ];
    throw new Error(`Unexpected command: ${command}`);
  });
  await act(async () => root.render(createElement(JiraSettings)));
  const button = (text: string) =>
    [...container.querySelectorAll("button")].find(
      (item) => item.textContent === text,
    )!;
  await input("Search Jira projects", "eng");
  expect(container.querySelectorAll('input[type="checkbox"]')).toHaveLength(2);
  await act(async () => button("Unselect all").click());
  expect(loadHiddenJiraProjectIds().sort()).toEqual(["1", "3"]);
  expect(container.textContent).toContain("1 of 3 selected");
  await input("Search Jira projects", "");
  await act(async () => button("Unselect all").click());
  expect(loadHiddenJiraProjectIds().sort()).toEqual(["1", "2", "3"]);
  await act(async () => button("Select all").click());
  expect(loadHiddenJiraProjectIds()).toEqual([]);
  await input("Search Jira projects", "nothing");
  expect(container.textContent).toContain("No projects match");
});
