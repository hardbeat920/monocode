import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { HARNESSES } from "../model/session";
import {
  HARNESS_ICONS,
  HarnessIcon,
  MONOCHROME_HARNESSES,
} from "./HarnessIcon";

it("provides an icon for every harness and valid monochrome entries", () => {
  expect(Object.keys(HARNESS_ICONS).sort()).toEqual([...HARNESSES].sort());
  for (const harness of HARNESSES) expect(HARNESS_ICONS[harness]).toBeTruthy();
  for (const harness of MONOCHROME_HARNESSES)
    expect(HARNESSES).toContain(harness);
  expect(MONOCHROME_HARNESSES.has("copilot")).toBe(true);
});

it("renders Copilot with quoted SVG masks for light and dark currentColor", () => {
  const markup = renderToStaticMarkup(
    createElement(HarnessIcon, { harness: "copilot" }),
  );
  expect(markup).toContain("bg-current");
  expect(markup).toContain("mask-image:url(&quot;");
  expect(markup).toContain("-webkit-mask-image:url(&quot;");
});
