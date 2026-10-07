// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { saveMaskEmails } from "../../features/settings/model/displayPrefs";
import { PrivateEmail } from "./PrivateEmail";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

function revealButton() {
  return container.querySelector<HTMLButtonElement>(
    '[aria-label="Reveal email"]',
  );
}

it("masks a revealed email again when masking is turned off and back on", async () => {
  await act(async () =>
    root.render(createElement(PrivateEmail, { email: "user@example.com" })),
  );
  await act(async () => revealButton()!.click());
  expect(container.querySelector('[aria-label="Hide email"]')).not.toBeNull();

  await act(async () => saveMaskEmails(false));
  expect(container.querySelector("button")).toBeNull();
  expect(container.textContent).toBe("user@example.com");

  await act(async () => saveMaskEmails(true));
  expect(container.querySelector('[aria-label="Hide email"]')).toBeNull();
  expect(revealButton()?.querySelector("span")?.className).toContain(
    "blur-[5px]",
  );
});

it("masks an email when another window turns masking on", async () => {
  saveMaskEmails(false);
  await act(async () =>
    root.render(createElement(PrivateEmail, { email: "user@example.com" })),
  );
  expect(revealButton()).toBeNull();

  await act(async () => {
    localStorage.setItem("monocode.maskEmails", "1");
    window.dispatchEvent(
      new StorageEvent("storage", { key: "monocode.maskEmails" }),
    );
  });

  expect(revealButton()?.querySelector("span")?.className).toContain(
    "blur-[5px]",
  );
});
