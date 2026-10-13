import { expect, it } from "vitest";
import { MEMORY_MAX_LINES } from "./monoFiles";
import {
  addMemoryEntry,
  archiveMemoryEntries,
  fitMemoryBudget,
  memoryEntry,
  memoryLines,
  withLineEdited,
  withoutLine,
  redactSecrets,
  removeMemoryEntry,
  searchMemory,
  sinceDate,
  supersedeMemoryEntry,
  topicName,
} from "./monoMemory";

it("writes one dated line per fact, with an optional end date", () => {
  expect(memoryEntry("  releases run\nfrom a v* tag ", "2026-10-04")).toBe(
    "- 2026-10-04 · releases run from a v* tag",
  );
  expect(memoryEntry("beta is open", "2026-10-04", "2026-11-01")).toBe(
    "- 2026-10-04 · beta is open · until 2026-11-01",
  );
  expect(() => memoryEntry("   ", "2026-10-04")).toThrow("empty");
  expect(() => memoryEntry("x", "2026-10-04", "next week")).toThrow("until");
  expect(() => memoryEntry("x".repeat(1001), "2026-10-04")).toThrow("under");
});

it("keeps credentials out of memory", () => {
  const text = redactSecrets(
    "deploy key ghp_abcdefghijklmnopqrstuvwxyz0123 and password: hunter22",
  );
  expect(text).not.toContain("ghp_abc");
  expect(text).not.toContain("hunter22");
  expect(text).toContain("«redacted");
  expect(redactSecrets("the token budget is 4k")).toBe(
    "the token budget is 4k",
  );
});

it("does not add a fact it already holds", () => {
  const once = addMemoryEntry("", "- 2026-10-01 · uses pnpm");
  expect(once).toEqual({ text: "- 2026-10-01 · uses pnpm\n", added: true });
  expect(addMemoryEntry(once.text, "- 2026-10-04 · uses pnpm").added).toBe(
    false,
  );
});

it("strikes a changed fact through instead of deleting it", () => {
  const text = "My notes\n- 2026-10-01 · CI runs on Travis\n";
  expect(
    supersedeMemoryEntry(
      text,
      "Travis",
      "- 2026-10-04 · CI runs on GitHub Actions",
      "2026-10-04",
    ),
  ).toBe(
    "My notes\n- ~~2026-10-01 · CI runs on Travis~~ · superseded 2026-10-04\n- 2026-10-04 · CI runs on GitHub Actions\n",
  );
});

it("changes only the one entry the text names", () => {
  const text =
    "- 2026-10-01 · api on port 3000\n- 2026-10-01 · web on port 3001\n";
  expect(() => removeMemoryEntry(text, "port")).toThrow("2 entries");
  expect(() => removeMemoryEntry(text, "nothing")).toThrow("No memory entry");
  expect(removeMemoryEntry(text, "web on")).toEqual({
    text: "- 2026-10-01 · api on port 3000\n",
    removed: "- 2026-10-01 · web on port 3001",
  });
});

it("archives struck, then expired, then oldest entries, never the user's own", () => {
  const filler = Array.from(
    { length: MEMORY_MAX_LINES - 2 },
    (_, i) => `- 2026-09-01 · fact ${i}`,
  );
  const text = [
    "User line without a date",
    "- ~~2026-08-01 · old~~ · superseded 2026-09-01",
    "- 2026-08-02 · beta · until 2026-09-30",
    ...filler,
    "- 2026-10-04 · newest",
    "- 2026-10-04 · just written",
  ].join("\n");
  const { text: fitted, moved } = fitMemoryBudget(
    text,
    "- 2026-10-04 · just written",
    "2026-10-04",
  );
  expect(moved).toEqual([
    "- ~~2026-08-01 · old~~ · superseded 2026-09-01",
    "- 2026-08-02 · beta · until 2026-09-30",
    "- 2026-09-01 · fact 0",
  ]);
  expect(fitted.split("\n").filter(Boolean)).toHaveLength(MEMORY_MAX_LINES);
  expect(fitted.startsWith("User line without a date\n")).toBe(true);
  expect(fitted).toContain("just written");
  expect(archiveMemoryEntries("", moved.slice(0, 1), "2026-10-04")).toBe(
    "# Archive\n\n- ~~2026-08-01 · old~~ · superseded 2026-09-01 · moved 2026-10-04\n",
  );
});

it("accepts plain topic names only", () => {
  expect(topicName("releases.md")).toBe("releases");
  for (const bad of ["", "../soul", ".hidden", "a/b", "archive"])
    expect(() => topicName(bad)).toThrow("topic");
});

it("finds entries by their words across files, newest first among equals", () => {
  const files = [
    {
      file: "MEMORY.md",
      text: "- 2026-09-01 · releases run from a v* tag\n- 2026-10-02 · the release checklist lives in docs\n",
    },
    {
      file: "memory/people.md",
      text: "# People\n\n- 2026-10-03 · Ana owns the release notes\n",
    },
    {
      file: "memory/archive.md",
      text: "- 2026-01-01 · releases ran from Jenkins · moved 2026-09-01\n",
    },
  ];
  expect(
    searchMemory(files, "Who handles the release?").map((hit) => hit.line),
  ).toEqual([
    "- 2026-10-03 · Ana owns the release notes",
    "- 2026-10-02 · the release checklist lives in docs",
    "- 2026-09-01 · releases run from a v* tag",
    "- 2026-01-01 · releases ran from Jenkins · moved 2026-09-01",
  ]);
  expect(searchMemory(files, "release", { since: "2026-10-01" })).toHaveLength(
    2,
  );
  expect(searchMemory(files, "deploy pipeline")).toEqual([]);
  expect(() => searchMemory(files, "is it")).toThrow("query");
});

it("reads since as a date or a span back from today", () => {
  const today = new Date(2026, 9, 4, 12);
  expect(sinceDate("7d", today)).toBe("2026-09-27");
  expect(sinceDate("2026-10-01", today)).toBe("2026-10-01");
  expect(() => sinceDate("last week", today)).toThrow("since");
});

it("lists memory as facts, whatever wrote them", () => {
  const text = [
    "# Memory",
    "",
    "i am a cool pirate",
    "- 2026-10-04 · The user's name is Nick",
    "- ~~2026-10-01 · CI runs on Travis~~ · superseded 2026-10-04",
    "- 2026-10-04 · beta is open · until 2026-11-01",
  ].join("\n");
  expect(memoryLines(text)).toEqual([
    { index: 2, text: "i am a cool pirate", struck: false },
    {
      index: 3,
      text: "The user's name is Nick",
      struck: false,
      date: "2026-10-04",
    },
    { index: 4, text: "CI runs on Travis", struck: true, date: "2026-10-01" },
    {
      index: 5,
      text: "beta is open",
      struck: false,
      date: "2026-10-04",
      until: "2026-11-01",
    },
  ]);
  expect(withoutLine(text, 2)).not.toContain("pirate");
});

it("rewords a fact in place, keeping its until", () => {
  const text = [
    "i am a cool pirate",
    "- 2026-10-01 · beta is open · until 2026-11-01",
    "",
  ].join("\n");
  expect(withLineEdited(text, 1, "beta is  closed", "2026-10-04")).toBe(
    "i am a cool pirate\n- 2026-10-04 · beta is closed · until 2026-11-01\n",
  );
  expect(withLineEdited(text, 0, "a calm sailor", "2026-10-04")).toBe(
    "a calm sailor\n- 2026-10-01 · beta is open · until 2026-11-01\n",
  );
  expect(() => withLineEdited(text, 0, "  ", "2026-10-04")).toThrow("empty");
});
