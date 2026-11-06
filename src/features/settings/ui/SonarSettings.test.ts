// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { SonarSettings } from "./SonarSettings";
import type { SonarStatus } from "../../inbox/model/sonar";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const PROJECTS = [
  { key: "org_api", name: "API service" },
  { key: "org_app", name: "Desktop app" },
  { key: "org_web", name: "Website" },
];
const disconnected = (): SonarStatus => ({
  connected: false,
  site: "",
  organization: "",
  projects: [],
  selectedProject: "",
});
let status: SonarStatus;
let organizations: { key: string; name: string }[];
let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.mocked(invoke).mockReset();
  organizations = [{ key: "org", name: "Org" }];
  status = disconnected();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const input = args as Record<string, unknown> | undefined;
    if (command === "sonar_status") return status;
    if (command === "sonar_connect") {
      if (input?.token === "bad-token")
        throw new Error("Sonar denied access. Use Browse permission");
      status = {
        ...disconnected(),
        connected: true,
        site: String(input?.site),
      };
      return status;
    }
    if (command === "sonar_disconnect") return (status = disconnected());
    if (command === "sonar_list_organizations") return organizations;
    if (command === "sonar_list_projects") return PROJECTS;
    if (command === "sonar_set_organization") {
      status = {
        ...status,
        organization: String(input?.organization),
        projects: [],
      };
      return status;
    }
    if (command === "sonar_set_projects") {
      const projects = input?.projects as SonarStatus["projects"];
      status = { ...status, projects, selectedProject: projects[0]?.key ?? "" };
      return status;
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
function checkbox(name: string) {
  return Array.from(container.querySelectorAll("label"))
    .find((label) => label.textContent === name)!
    .querySelector<HTMLInputElement>('input[type="checkbox"]')!;
}
async function click(element: HTMLElement) {
  await act(async () => element.click());
}
it("connects with only a token, picks the sole organization, and edits watched projects without reconnecting", async () => {
  await act(async () => root.render(createElement(SonarSettings)));
  expect(
    container.querySelector('[aria-label="Sonar project keys"]'),
  ).toBeNull();
  await input("Sonar personal access token", "test-token");
  await submit();
  expect(invoke).toHaveBeenCalledWith("sonar_connect", {
    site: "https://sonarcloud.io",
    token: "test-token",
  });
  expect(container.innerHTML).not.toContain("test-token");
  expect(invoke).toHaveBeenCalledWith("sonar_set_organization", {
    organization: "org",
  });
  expect(container.textContent).toContain("0 of 3");
  await click(checkbox("Website"));
  await click(checkbox("API service"));
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: [PROJECTS[0], PROJECTS[2]],
  });
  await click(checkbox("Website"));
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: [PROJECTS[0]],
  });
  expect(checkbox("API service").checked).toBe(true);
  expect(checkbox("Website").checked).toBe(false);
  expect(
    vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "sonar_connect"),
  ).toHaveLength(1);
  await click(
    Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Disconnect",
    )!,
  );
  expect(invoke).toHaveBeenLastCalledWith("sonar_disconnect", undefined);
  expect(container.textContent).toContain("Connect Sonar");
});
it("lists SonarQube Server projects without an organization and searches them", async () => {
  organizations = [];
  status = {
    ...disconnected(),
    connected: true,
    site: "https://sonar.example",
  };
  await act(async () => root.render(createElement(SonarSettings)));
  expect(
    container.querySelector('[aria-label="Sonar organization"]'),
  ).toBeNull();
  expect(invoke).not.toHaveBeenCalledWith(
    "sonar_set_organization",
    expect.anything(),
  );
  await input("Search Sonar projects", "web");
  expect(
    Array.from(container.querySelectorAll("li label")).map(
      (label) => label.textContent,
    ),
  ).toEqual(["Website"]);
});
it("keeps the connection form available when Browse access fails", async () => {
  await act(async () => root.render(createElement(SonarSettings)));
  await input("Sonar personal access token", "bad-token");
  await submit();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Browse permission",
  );
  expect(container.querySelector("form")).not.toBeNull();
});
it("watches or unwatches every listed project at once, scoped to the search", async () => {
  status = {
    ...disconnected(),
    connected: true,
    site: "https://sonarcloud.io",
    organization: "org",
    projects: [PROJECTS[1]!],
  };
  await act(async () => root.render(createElement(SonarSettings)));
  const all = () =>
    container.querySelector<HTMLInputElement>(
      'input[aria-label$="all listed projects"]',
    )!;
  expect(all().checked).toBe(false);
  expect(all().indeterminate).toBe(true);
  await click(all());
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: PROJECTS,
  });
  expect(all().checked).toBe(true);
  await input("Search Sonar projects", "web");
  await click(all());
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: [PROJECTS[0], PROJECTS[1]],
  });
  await input("Search Sonar projects", "");
  await click(all());
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: PROJECTS,
  });
  await click(all());
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: [],
  });
  expect(container.textContent).toContain("0 of 3");
});
const connectedTo = (organization = ""): SonarStatus => ({
  ...disconnected(),
  connected: true,
  site: "https://sonarcloud.io",
  organization,
});
it("waits for a manual pick when the token belongs to several organizations", async () => {
  organizations = [
    { key: "org", name: "Org" },
    { key: "other", name: "Other" },
  ];
  status = connectedTo();
  await act(async () => root.render(createElement(SonarSettings)));
  expect(invoke).not.toHaveBeenCalledWith(
    "sonar_set_organization",
    expect.anything(),
  );
  expect(invoke).not.toHaveBeenCalledWith("sonar_list_projects");
  await click(
    container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Sonar organization"]',
    )!,
  );
  await click(
    Array.from(
      document.body.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ).find((option) => option.textContent === "Other")!,
  );
  expect(invoke).toHaveBeenCalledWith("sonar_set_organization", {
    organization: "other",
  });
  expect(invoke).toHaveBeenCalledWith("sonar_list_projects");
  expect(container.textContent).toContain("0 of 3");
});
it("reports an organization switch failure and keeps the watched projects", async () => {
  organizations = [
    { key: "org", name: "Org" },
    { key: "other", name: "Other" },
  ];
  status = { ...connectedTo("org"), projects: [PROJECTS[1]!] };
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_set_organization")
      throw new Error("Could not reach Sonar");
    return base(command, args);
  });
  await act(async () => root.render(createElement(SonarSettings)));
  await click(
    container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Sonar organization"]',
    )!,
  );
  await click(
    Array.from(
      document.body.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ).find((option) => option.textContent === "Other")!,
  );
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Could not reach Sonar",
  );
  expect(checkbox("Desktop app").checked).toBe(true);
});
it("reports a project list failure", async () => {
  status = connectedTo("org");
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_list_projects")
      throw new Error("Sonar denied access");
    return base(command, args);
  });
  await act(async () => root.render(createElement(SonarSettings)));
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Sonar denied access",
  );
  expect(container.textContent).toContain("No projects are visible");
});
it("keeps the checkboxes on the stored projects when a save fails", async () => {
  status = { ...connectedTo("org"), projects: [PROJECTS[1]!] };
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_set_projects") throw new Error("Disk full");
    return base(command, args);
  });
  await act(async () => root.render(createElement(SonarSettings)));
  await click(checkbox("Website"));
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Disk full",
  );
  expect(checkbox("Website").checked).toBe(false);
  expect(checkbox("Desktop app").checked).toBe(true);
});
it("keeps a watched project the token can no longer see so it can be unwatched", async () => {
  const gone = { key: "org_gone", name: "Retired service" };
  status = { ...connectedTo("org"), projects: [gone] };
  await act(async () => root.render(createElement(SonarSettings)));
  expect(checkbox("Retired service").checked).toBe(true);
  await click(checkbox("Website"));
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: [PROJECTS[2], gone],
  });
  await click(checkbox("Retired service"));
  expect(invoke).toHaveBeenLastCalledWith("sonar_set_projects", {
    projects: [PROJECTS[2]],
  });
});
it("does not fall back to listing every project when organizations cannot be read", async () => {
  status = connectedTo();
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_list_organizations")
      throw new Error("Could not reach Sonar");
    return base(command, args);
  });
  await act(async () => root.render(createElement(SonarSettings)));
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Could not reach Sonar",
  );
  expect(invoke).not.toHaveBeenCalledWith("sonar_list_projects");
  expect(container.textContent).not.toContain("Loading organizations");
});
