// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  DateTimePicker,
  parseLocalDateTime,
  toLocalDateTime,
} from "./DateTimePicker";

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(Date, "now").mockReturnValue(new Date(2028, 1, 20, 10).getTime());
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

function day(date: string) {
  const button = container.querySelector<HTMLButtonElement>(
    `button[aria-label="${date}"]`,
  );
  expect(button, date).not.toBeNull();
  return button!;
}

it("selects a leap day while keeping the chosen local time", () => {
  const onChange = vi.fn();
  act(() =>
    root.render(
      createElement(DateTimePicker, { value: "2028-02-20T18:30", onChange }),
    ),
  );
  act(() => day("2028-02-29").click());
  expect(onChange).toHaveBeenCalledWith("2028-02-29T18:30");
  expect(container.querySelector('input[type="datetime-local"]')).toBeNull();
  expect(document.activeElement).not.toBe(day("2028-02-20"));
});

function clickControl(label: string) {
  act(() =>
    container
      .querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!
      .click(),
  );
}

function key(key: string) {
  act(() =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    ),
  );
}

it("navigates the calendar by keyboard with one tab stop, month clamping and minimum-date bounds", () => {
  const onChange = vi.fn();
  act(() =>
    root.render(
      createElement(DateTimePicker, {
        value: "2028-01-31T08:15",
        minDate: "2028-01-30",
        autoFocus: true,
        onChange,
      }),
    ),
  );
  expect(document.activeElement).toBe(day("2028-01-31"));
  expect(
    container.querySelectorAll('[role="grid"] button[tabindex="0"]'),
  ).toHaveLength(1);
  key("PageDown");
  expect(document.activeElement).toBe(day("2028-02-29"));
  key("ArrowRight");
  expect(document.activeElement).toBe(day("2028-03-01"));
  key("ArrowUp");
  expect(document.activeElement).toBe(day("2028-02-23"));
  key("Home");
  expect(document.activeElement).toBe(day("2028-02-21"));
  key("End");
  expect(document.activeElement).toBe(day("2028-02-27"));
  key("PageUp");
  expect(document.activeElement).toBe(day("2028-01-30"));
  key("ArrowLeft");
  expect(document.activeElement).toBe(day("2028-01-30"));
  expect(onChange).not.toHaveBeenCalled();
  key("Enter");
  expect(onChange).toHaveBeenLastCalledWith("2028-01-30T08:15");
  key("ArrowRight");
  key(" ");
  expect(onChange).toHaveBeenLastCalledWith("2028-01-31T08:15");
});

it("browses across year boundaries without selecting a date and disables days before the minimum", () => {
  const onChange = vi.fn();
  act(() =>
    root.render(
      createElement(DateTimePicker, {
        value: "2028-12-25T09:10",
        minDate: "2028-12-20",
        onChange,
      }),
    ),
  );
  expect(day("2028-12-19").disabled).toBe(true);
  expect(day("2028-12-20").disabled).toBe(false);
  clickControl("Next month");
  expect(day("2029-01-01").disabled).toBe(false);
  expect(onChange).not.toHaveBeenCalled();
  clickControl("Previous month");
  expect(day("2028-12-25").disabled).toBe(false);
  expect(
    container.querySelector<HTMLButtonElement>(
      'button[aria-label="Previous month"]',
    )!.disabled,
  ).toBe(true);
});

it("keeps keyboard navigation available when the minimum date advances into another month", () => {
  const props = {
    value: "2028-02-29T08:15",
    minDate: "2028-02-29",
    autoFocus: true,
    onChange: vi.fn(),
  };
  act(() => root.render(createElement(DateTimePicker, props)));
  expect(document.activeElement).toBe(day("2028-02-29"));
  act(() =>
    root.render(
      createElement(DateTimePicker, { ...props, minDate: "2028-03-01" }),
    ),
  );
  expect(day("2028-03-01").disabled).toBe(false);
  expect(document.activeElement).toBe(day("2028-03-01"));
  expect(
    container.querySelectorAll('[role="grid"] button[tabindex="0"]'),
  ).toHaveLength(1);
  expect(props.onChange).not.toHaveBeenCalled();
});

it("round-trips valid local dates and rejects malformed or normalized values", () => {
  const date = parseLocalDateTime("2028-02-29T23:45");
  expect(date).toEqual(new Date(2028, 1, 29, 23, 45));
  expect(toLocalDateTime(date!)).toBe("2028-02-29T23:45");
  for (const invalid of [
    "2027-02-29T12:00",
    "2028-04-31T12:00",
    "2028-01-01T24:00",
    "2028-01-01T12:60",
    "2028-01-01T",
    "2028-01-01T1:00",
    "2028-01-01T12:00Z",
    "",
  ]) {
    expect(parseLocalDateTime(invalid), invalid).toBeNull();
  }
});
