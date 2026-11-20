import { clearProjectRepoNames } from "../model/inboxProjectRepos";
// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { SonarIssueDetail } from "./SonarIssueDetail";
import {
  clearSonarContent,
  sonarIssueToInboxItem,
  type SonarIssueContent,
} from "../model/sonar";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("../../sessions/ui/AgentMarkdown", () => ({
  AgentMarkdown: ({ text }: { text: string }) =>
    createElement("div", null, text),
}));
vi.mock("../../files/editor/editorLanguage", () => ({
  languageForPath: async () => null,
}));
vi.mock("../../source-control/ui/UnifiedDiffView", () => ({
  UnifiedDiffView: ({ files }: { files: unknown[] }) =>
    createElement("div", { "data-example-diff": true }, JSON.stringify(files)),
}));
const item = sonarIssueToInboxItem({
  key: "i",
  projectKey: "org_repository",
  repositoryName: "repository",
  branch: "develop",
  message: "Check ModelState.IsValid",
  severity: "CRITICAL",
  rule: "csharpsquid:S6967",
  issueType: "CODE_SMELL",
  status: "OPEN",
  resolved: false,
  file: "Controller.cs",
  line: 12,
  updatedAt: "2026-10-14",
  assignee: "",
  url: "https://sonarcloud.io/project/issues?id=org_repository",
});
const content: SonarIssueContent = {
  repositoryName: "repository",
  comparison: {
    baseBranch: "main",
    original: "Save();",
    current: "if (ModelState.IsValid) Save();",
  },
  issue: {
    textRange: { startLine: 12, endLine: 12 },
    effort: "5min",
    flows: [
      {
        locations: [
          {
            component: "org_repository:Controller.cs",
            msg: "Related action",
            textRange: { startLine: 10, endLine: 10 },
          },
        ],
      },
    ],
  },
  rule: { name: "Validate model", lang: "cs" },
  ruleHtml:
    '<h2>Why is this an issue?</h2><p>Invalid models can corrupt data.</p><h2>How to fix it</h2><pre data-diff-id="1" data-diff-type="noncompliant">Save();</pre><pre data-diff-id="1" data-diff-type="compliant">if (ModelState.IsValid) Save();</pre>',
  ruleUrl: "https://github.com/SonarSource/sonar-dotnet",
  referenceDocumentation: true,
  source: [
    { line: 11, code: "void Create() {" },
    { line: 12, code: "Save();" },
    { line: 13, code: "}" },
  ],
  errors: [],
};
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  clearSonarContent();
  clearProjectRepoNames();
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
it("shows repository name, highlighted source, rule guidance and example diff, and sends that context to a session", async () => {
  vi.mocked(invoke).mockResolvedValue(content);
  const onStart = vi.fn();
  await act(async () =>
    root.render(
      createElement(SonarIssueDetail, {
        item,
        cwd: "/tmp/app",
        projects: [{ path: "/tmp/app", name: "App" }],
        onStart,
      }),
    ),
  );
  expect(container.textContent).toContain("Sonar · repository · develop");
  expect(container.textContent).toContain("Critical");
  expect(container.textContent).toContain("Code smell");
  expect(container.textContent).not.toContain("CODE_SMELL");
  expect(
    container.querySelector('[aria-label="Issue at line 12"]')?.textContent,
  ).toContain("Save();");
  expect(container.textContent).toContain("Invalid models can corrupt data");
  expect(container.textContent).toContain("How to fix it");
  expect(container.textContent).toContain("Related action");
  expect(container.querySelectorAll("[data-example-diff]")).toHaveLength(2);
  expect(container.textContent).toContain("main → develop");
  expect(container.textContent).toContain(
    "Latest official Sonar analyzer documentation",
  );
  await act(async () => {
    Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent === "Start session")!
      .click();
  });
  expect(onStart.mock.lastCall![0].projectPath).toBe("/tmp/app");
  expect(onStart.mock.lastCall![1]).toContain(
    "Invalid models can corrupt data",
  );
  expect(onStart.mock.lastCall![1]).toContain("12: Save();");
});
it("keeps the heading available on a failed detail load and offers retry", async () => {
  vi.mocked(invoke)
    .mockRejectedValueOnce(new Error("Sonar access denied"))
    .mockResolvedValue(content);
  await act(async () =>
    root.render(
      createElement(SonarIssueDetail, { item, cwd: "/tmp/app", projects: [] }),
    ),
  );
  expect(container.textContent).toContain("Sonar access denied");
  expect(container.textContent).toContain("Check ModelState.IsValid");
  await act(async () => {
    Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
      .find((button) => button.textContent === "Retry")!
      .click();
  });
  expect(container.textContent).toContain("Invalid models can corrupt data");
});

it.each([
  {
    name: "matches the project display name",
    projects: [
      { path: "/tmp/unrelated", name: "Other" },
      { path: "/tmp/checkout", name: "Repository" },
    ],
    expected: "/tmp/checkout",
  },
  {
    name: "matches the folder when the project has a custom name",
    projects: [
      { path: "/tmp/unrelated", name: "Other" },
      { path: "/tmp/repository", name: "Custom name" },
    ],
    expected: "/tmp/repository",
  },
  {
    name: "keeps the current project when no repository matches",
    projects: [
      { path: "/tmp/first", name: "First" },
      { path: "/tmp/unrelated", name: "Other" },
    ],
    expected: "/tmp/unrelated",
  },
  {
    name: "keeps the first project fallback when no repository or current project matches",
    projects: [{ path: "/tmp/first", name: "First" }],
    expected: "/tmp/first",
  },
])("$name", async ({ projects, expected }) => {
  vi.mocked(invoke).mockResolvedValue(content);
  await act(async () =>
    root.render(
      createElement(SonarIssueDetail, {
        item,
        cwd: "/tmp/unrelated",
        projects,
        onStart: vi.fn(),
      }),
    ),
  );
  expect(
    container.querySelector<HTMLSelectElement>('[aria-label="Work in project"]')
      ?.value,
  ).toBe(expected);
});

it("matches a repository name discovered later without replacing a manual choice", async () => {
  let resolve!: (value: SonarIssueContent) => void;
  vi.mocked(invoke).mockReturnValue(
    new Promise<SonarIssueContent>((done) => {
      resolve = done;
    }),
  );
  const projects = [
    { path: "/tmp/current", name: "Current" },
    { path: "/tmp/repository", name: "Repo" },
    { path: "/tmp/chosen", name: "Chosen" },
  ];
  await act(async () =>
    root.render(
      createElement(SonarIssueDetail, {
        item: {
          ...item,
          projectName: "org_repository",
        },
        cwd: "/tmp/current",
        projects,
        onStart: vi.fn(),
      }),
    ),
  );
  const select = container.querySelector<HTMLSelectElement>(
    '[aria-label="Work in project"]',
  )!;
  expect(select.value).toBe("/tmp/current");
  await act(async () => {
    select.value = "/tmp/chosen";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => {
    resolve(content);
  });
  expect(select.value).toBe("/tmp/chosen");
});

it("prefers a Git remote match when both the folder and project display name differ", async () => {
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "git_repository_names")
      return (args as { cwd: string }).cwd === "/tmp/custom-checkout"
        ? ["fork", "repository"]
        : [];
    return content;
  });
  await act(async () =>
    root.render(
      createElement(SonarIssueDetail, {
        item,
        cwd: "/tmp/repository",
        projects: [
          { path: "/tmp/repository", name: "Repository" },
          { path: "/tmp/custom-checkout", name: "Custom project" },
        ],
        onStart: vi.fn(),
      }),
    ),
  );
  expect(
    container.querySelector<HTMLSelectElement>('[aria-label="Work in project"]')
      ?.value,
  ).toBe("/tmp/custom-checkout");
});

it("keeps a manual selection when Git remote matching finishes later", async () => {
  let resolve!: (names: string[]) => void;
  const pending = new Promise<string[]>((done) => {
    resolve = done;
  });
  vi.mocked(invoke).mockImplementation(async (command) =>
    command === "git_repository_names" ? pending : content,
  );
  await act(async () =>
    root.render(
      createElement(SonarIssueDetail, {
        item,
        cwd: "/tmp/current",
        projects: [
          { path: "/tmp/current", name: "Current" },
          { path: "/tmp/custom", name: "Custom" },
        ],
        onStart: vi.fn(),
      }),
    ),
  );
  const select = container.querySelector<HTMLSelectElement>(
    '[aria-label="Work in project"]',
  )!;
  await act(async () => {
    select.value = "/tmp/custom";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => {
    resolve(["repository"]);
  });
  expect(select.value).toBe("/tmp/custom");
});
