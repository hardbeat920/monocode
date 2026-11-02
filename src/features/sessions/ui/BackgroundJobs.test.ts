// @vitest-environment happy-dom
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { BackgroundJobs } from "./BackgroundJobs";

const jobs = [
  {
    id: "b1",
    title: "Start the dev server",
    command: "npm run dev",
    startedAt: Date.now() - 125_000,
  },
  { id: "a1", title: "Review the auth module", startedAt: Date.now() - 4_000 },
];

it("lists each job with how long it has run, and a stop button", () => {
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(
    createElement(BackgroundJobs, { jobs, onStop: vi.fn() }),
  );
  expect(container.textContent).toContain("Start the dev server");
  expect(container.textContent).toContain("npm run dev");
  expect(container.textContent).toContain("2m 5s");
  expect(container.textContent).toContain("Review the auth module");
  expect(
    container.querySelector('[aria-label="Stop Review the auth module"]'),
  ).not.toBeNull();
});

it("takes no room without jobs, and offers no stop it can't do", () => {
  expect(renderToStaticMarkup(createElement(BackgroundJobs, {}))).toBe("");
  expect(
    renderToStaticMarkup(createElement(BackgroundJobs, { jobs })),
  ).not.toContain("aria-label");
});
