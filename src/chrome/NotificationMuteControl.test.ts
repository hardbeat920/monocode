// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import { NotificationMuteControl } from "./NotificationMuteControl";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  vi.spyOn(Date, "now").mockReturnValue(1_800_000_000_000);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function click(label: string) {
  const button = [...container.querySelectorAll("button")].find(
    (item) => item.textContent === label,
  );
  expect(button, `Button ${label}`).toBeDefined();
  act(() => button!.click());
}

describe("NotificationMuteControl", () => {
  it("mutes selected projects for eight hours without replacing category choices", () => {
    updateNotificationPreferences(["private"], { disabled: ["issues"] });
    act(() =>
      root.render(
        createElement(NotificationMuteControl, {
          projectIds: ["private", "personal"],
        }),
      ),
    );
    click("8 hours");
    expect(loadNotificationPreferences()).toEqual({
      private: { disabled: ["issues"], mutedUntil: 1_800_028_800_000 },
      personal: { disabled: [], mutedUntil: 1_800_028_800_000 },
    });
  });

  it("resumes notifications while preserving the project's chosen categories", () => {
    updateNotificationPreferences(["private"], {
      disabled: ["issues"],
      mutedUntil: null,
    });
    act(() =>
      root.render(
        createElement(NotificationMuteControl, { projectIds: ["private"] }),
      ),
    );
    expect(container.textContent).toContain("Muted until resumed");
    click("Resume notifications");
    expect(loadNotificationPreferences().private.disabled).toEqual(["issues"]);
    expect(loadNotificationPreferences().private.mutedUntil).toBeUndefined();
    expect(container.textContent).not.toContain("Muted until resumed");
  });

  it("accepts a custom future time and rejects an expired one without saving", () => {
    vi.mocked(Date.now).mockReturnValue(new Date(2030, 0, 15, 12).getTime());
    act(() =>
      root.render(
        createElement(NotificationMuteControl, { projectIds: ["private"] }),
      ),
    );
    click("Choose date and time");
    expect(
      [...container.querySelectorAll("button")].some(
        (button) => button.textContent === "1 hour",
      ),
    ).toBe(false);
    expect(container.querySelector('input[type="datetime-local"]')).toBeNull();
    expect(container.querySelector('[role="grid"]')).not.toBeNull();
    act(() =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="2030-01-15"]')!
        .click(),
    );
    const input = container.querySelector<HTMLInputElement>(
      'input[placeholder="HH:mm"]',
    );
    expect(input).not.toBeNull();
    const setter = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!;
    const type = (value: string) =>
      act(() => {
        setter.call(input, value);
        input!.dispatchEvent(new Event("input", { bubbles: true }));
      });
    type("08:00");
    click("Mute until then");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "future",
    );
    expect(loadNotificationPreferences().private).toBeUndefined();
    act(() =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="2030-01-16"]')!
        .click(),
    );
    type("17:00");
    const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("Storage full");
    });
    click("Mute until then");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "Could not save",
    );
    expect(input!.value).toBe("17:00");
    write.mockRestore();
    click("Mute until then");
    expect(loadNotificationPreferences().private.mutedUntil).toBe(
      new Date(2030, 0, 16, 17).getTime(),
    );
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(document.activeElement?.textContent).toBe("Choose date and time");
  });
});
