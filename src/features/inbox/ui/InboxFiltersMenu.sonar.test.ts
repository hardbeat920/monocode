// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { InboxFiltersMenu } from "./InboxFiltersMenu";
import {
  DEFAULT_INBOX_FILTERS,
  type InboxFilters,
} from "../model/inboxFilters";
import { sonarBranchKey } from "../model/sonar";
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
it("offers every scanned branch without deriving the options from visible issues", async () => {
  const onChange = vi.fn();
  const branches = [
    {
      projectKey: "app",
      repositoryName: "monocode",
      name: "main",
      isMain: true,
      analysisDate: "2026-10-14",
    },
    {
      projectKey: "app",
      repositoryName: "monocode",
      name: "zero-issues",
      isMain: false,
      analysisDate: "2026-10-14",
    },
    {
      projectKey: "other",
      name: "main",
      isMain: true,
      analysisDate: "2026-10-14",
    },
  ];
  const render = (filters: InboxFilters) =>
    act(async () =>
      root.render(
        createElement(InboxFiltersMenu, {
          x: 0,
          y: 0,
          projects: [],
          linearProjects: [],
          linearTeams: [],
          hiddenLinearTeamIds: [],
          jiraProjects: [],
          hiddenJiraProjectIds: [],
          source: "sonar",
          sonarBranches: branches,
          filters,
          onChange,
          onLinearTeamsChange: vi.fn(),
          onJiraProjectsChange: vi.fn(),
          onClose: vi.fn(),
        }),
      ),
    );
  await render(DEFAULT_INBOX_FILTERS);
  const row = () =>
    Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="menuitemcheckbox"]'),
    ).find((button) => button.textContent?.includes("zero-issues"))!;
  expect(row().getAttribute("aria-checked")).toBe("true");
  expect(document.body.textContent).toContain("monocode");
  expect(document.querySelector('[title="app"]')?.textContent).toContain(
    "monocode",
  );
  expect(document.body.textContent).toContain("other");
  expect(document.querySelectorAll('[title="main (main)"]')).toHaveLength(2);
  await act(async () => row().click());
  expect(onChange).toHaveBeenLastCalledWith({
    ...DEFAULT_INBOX_FILTERS,
    hiddenSonarBranches: [sonarBranchKey("app", "zero-issues")],
  });
  await render(onChange.mock.lastCall![0]);
  expect(row().getAttribute("aria-checked")).toBe("false");
  await act(async () => row().click());
  expect(onChange.mock.lastCall![0].hiddenSonarBranches).toEqual([]);
});
