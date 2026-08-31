import { describe, expect, it } from "vitest";
import {
  releaseNotesForVersion,
  releaseNotesMarkdown,
  releaseNotesTitle,
} from "./releaseNotes";

const fixture = `# Changelog

## [Unreleased]

### Added
- Future work.

## [0.1.3] - 2026-09-01

### Fixed
- Newer fix.

## [0.1.2] - 2026-08-31

### Added
- Requested feature.

## [0.1.1]

### Fixed
- Older fix.
`;

describe("releaseNotesForVersion", () => {
  it("extracts only the requested release", () => {
    const release = releaseNotesForVersion("0.1.2", fixture);

    expect(release?.source).toEqual({ version: "0.1.2" });
    expect(releaseNotesTitle(release!.source.version)).toBe(
      "What's new in MonoCode 0.1.2",
    );
    expect(release?.markdown).toContain("## [0.1.2]");
    expect(release?.markdown).not.toContain("## [0.1.3]");
    expect(release?.markdown).not.toContain("## [0.1.1]");
  });

  it.each(["", "   ", "Unreleased", "9.9.9"])(
    "returns null for unavailable version %j",
    (version) => {
      expect(releaseNotesForVersion(version, fixture)).toBeNull();
    },
  );

  it("requires the heading to occupy the complete line", () => {
    const malformed = `## Prefix [0.1.2]\nNo.\n\n## [0.1.2] soon\nStill no.`;
    expect(releaseNotesForVersion("0.1.2", malformed)).toBeNull();
  });

  it.each(["## [0.1.2]\n\nUndated.", "## [0.1.2] - 2026-08-31\n\nDated."])(
    "accepts supported heading %j",
    (changelog) => {
      expect(releaseNotesForVersion("0.1.2", changelog)?.markdown).toBe(
        changelog,
      );
    },
  );

  it("extracts the final release through the end of the changelog", () => {
    const release = releaseNotesForVersion("0.1.1", fixture);
    expect(release?.markdown).toContain("Older fix.");
  });

  it("escapes the requested version before matching", () => {
    expect(
      releaseNotesForVersion("0.1.2+test", "## [0.1.2+test]\n\nExact."),
    ).toEqual({
      source: { version: "0.1.2+test" },
      markdown: "## [0.1.2+test]\n\nExact.",
    });
  });
});

describe("releaseNotesMarkdown", () => {
  it("resolves a stored source against the bundled changelog shape", () => {
    const release = releaseNotesForVersion("0.1.2", fixture);
    expect(releaseNotesMarkdown(release!.source, fixture)).toBe(
      release?.markdown,
    );
  });
});
