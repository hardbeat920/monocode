import { expect, it } from "vitest";

// WebKit re-applies :focus-visible to the focused element on every key press.
// For a :has() that is not on the styled element itself (group-has-*,
// peer-has-*), Safari 26's WebKit then re-checks every element in the page.
// A bare `:has(:focus-visible)` matches the composer's textarea too, so each
// keystroke walked the whole transcript. Naming the element keeps typing out.
const sources = import.meta.glob(
  ["../../**/*.{ts,tsx}", "!../../**/*.test.{ts,tsx}"],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

it("keeps a bare :focus-visible out of group-has and peer-has variants", () => {
  const offenders = Object.keys(sources).filter((path) =>
    /\b(?:group|peer)-has-\[:focus-visible\]/.test(sources[path]),
  );
  expect(Object.keys(sources).length).toBeGreaterThan(100);
  expect(offenders).toEqual([]);
});
