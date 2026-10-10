// @vitest-environment happy-dom
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { UsageLimitNotice } from "./UsageLimitNotice";
import { UsageLimitAccountPicker } from "./UsageLimitAccountPicker";
import { saveProviderAccount } from "../../providers/model/providerAccounts";

vi.mock("../../../shared/ui/Popover", () => ({
  Popover: ({
    children,
    role,
    "aria-label": label,
  }: {
    children: ReactNode;
    role?: string;
    "aria-label"?: string;
  }) => createElement("div", { role, "aria-label": label }, children),
}));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function render(onSelect: (accountId: string) => void) {
  act(() =>
    root.render(
      createElement(UsageLimitNotice, {
        limit: { resetsAt: Date.now() + 3600_000 },
        modelPicker: createElement(UsageLimitAccountPicker, {
          harness: "claude",
          providerAccountId: undefined,
          label: "Switch account",
          onSelect,
        }),
      }),
    ),
  );
}

it("offers the composer banner's other accounts and routes the choice", () => {
  saveProviderAccount({
    id: "account-work",
    provider: "claude",
    label: "Work account",
  });
  const onSelect = vi.fn();
  render(onSelect);
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Switch account"]')!
      .click(),
  );
  expect(
    container.querySelector('[role="menuitem"][aria-label="Default account"]'),
  ).toBeNull();
  act(() =>
    container
      .querySelector<HTMLButtonElement>(
        '[role="menuitem"][aria-label="Work account"]',
      )!
      .click(),
  );
  expect(onSelect).toHaveBeenCalledWith("account-work");
});

it("stays out of the banner when no other account is saved", () => {
  render(vi.fn());
  expect(container.querySelector('[aria-label="Switch account"]')).toBeNull();
  expect(container.textContent).toContain("Resume at reset");
});
