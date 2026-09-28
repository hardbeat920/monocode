// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  QuickProjectIcon,
  loadQuickProjectAppearance,
} from "./QuickProjectIcon";
import { projectKey, projectName } from "../../../shared/lib/paths";
import { projectMascot } from "../../projects/model/projectMascots";

vi.mock("../../projects/model/projectLogos", () => ({
  projectLogoSrc: (path: string | null) => (path ? `asset://${path}` : null),
}));

const path = "/Users/me/code/agent-terminal";
let container: HTMLDivElement;
let root: Root;
let appearance: ReturnType<typeof loadQuickProjectAppearance>;
function render() {
  act(() =>
    root.render(
      createElement(QuickProjectIcon, {
        projectPath: path,
        appearance,
        className: "size-4",
      }),
    ),
  );
}
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  appearance = { logos: {}, mascots: {}, colors: {}, customColors: {} };
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("renders a mascot instead of trying to load a project directory as an image", () => {
  render();
  expect(container.querySelector("img")).toBeNull();
  expect(container.querySelector("svg path")?.getAttribute("d")).toBe(
    projectMascot(projectName(path)).restPath,
  );
});

it("uses the project's saved mascot and color", () => {
  appearance.mascots[projectKey(path)] = "cat";
  appearance.customColors[projectKey(path)] = "#ff0000";
  render();
  expect(container.querySelector("svg path")?.getAttribute("d")).toBe(
    projectMascot(projectName(path), "cat").restPath,
  );
  expect(container.querySelector("svg")?.style.color).toBe("#ff0000");
});

it("loads only saved logos and falls back to the mascot if the image fails", () => {
  appearance.logos[projectKey(path)] = "/app-data/logos/project.png";
  render();
  const image = container.querySelector("img")!;
  expect(image.getAttribute("src")).toBe("asset:///app-data/logos/project.png");
  act(() => image.dispatchEvent(new Event("error")));
  expect(container.querySelector("img")).toBeNull();
  expect(container.querySelector("svg")).not.toBeNull();
  appearance = { ...appearance };
  render();
  expect(container.querySelector("img")).not.toBeNull();
});
