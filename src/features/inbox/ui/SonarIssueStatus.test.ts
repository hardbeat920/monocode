// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { SonarIssueStatus } from "./SonarIssueStatus";
import { sonarIssueToInboxItem } from "../model/sonar";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const issue = {
  key: "i",
  projectKey: "org_app",
  repositoryName: "App",
  branch: "develop",
  message: "Check validation",
  severity: "CRITICAL",
  rule: "cs:S1",
  issueType: "CODE_SMELL",
  status: "OPEN",
  resolved: false,
  file: "Controller.cs",
  line: 3,
  updatedAt: "2026-10-14",
  assignee: "",
  url: "https://sonarcloud.io/project/issues?id=org_app",
};
const item = sonarIssueToInboxItem(issue);
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(invoke).mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
async function choose(value: string) {
  await act(async () => {
    const select = container.querySelector<HTMLSelectElement>("select")!;
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
it("offers only allowed transitions, and sends the optional comment with feedback disabled by default", async () => {
  const changed = vi.fn();
  vi.mocked(invoke).mockResolvedValue({
    ...issue,
    status: "ACCEPTED",
    resolved: true,
  });
  await act(async () =>
    root.render(
      createElement(SonarIssueStatus, {
        item,
        status: "OPEN",
        transitions: ["accept", "confirm"],
        loading: false,
        onChanged: changed,
      }),
    ),
  );
  expect(
    Array.from(container.querySelectorAll("option")).map(
      (option) => option.textContent,
    ),
  ).toEqual(["Open", "Accept", "Confirm"]);
  await choose("accept");
  const dialog = document.querySelector('[role="dialog"]')!;
  expect(
    dialog.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked,
  ).toBe(false);
  await act(async () => {
    const textarea = dialog.querySelector("textarea")!;
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(textarea, "Will fix later");
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => {
    dialog
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
  expect(invoke).toHaveBeenCalledWith("sonar_transition_issue", {
    request: {
      projectKey: "org_app",
      branch: "develop",
      issueKey: "i",
      transition: "accept",
      comment: "Will fix later",
      shareFeedback: false,
    },
  });
  expect(changed.mock.lastCall![0].state).toBe("closed");
  expect(changed.mock.lastCall![0].projectName).toBe("App");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});
it("keeps the dialog and comment when the API rejects the change", async () => {
  vi.mocked(invoke).mockRejectedValue(
    new Error("Sonar denied this status change"),
  );
  const changed = vi.fn();
  await act(async () =>
    root.render(
      createElement(SonarIssueStatus, {
        item,
        status: "OPEN",
        transitions: ["falsepositive"],
        loading: false,
        onChanged: changed,
      }),
    ),
  );
  await choose("falsepositive");
  await act(async () => {
    document
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Sonar denied",
  );
  expect(changed).not.toHaveBeenCalled();
});
