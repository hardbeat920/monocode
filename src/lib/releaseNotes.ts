import bundledChangelog from "../../CHANGELOG.md?raw";

export type ReleaseNotesTabSource = {
  version: string;
};

export type ReleaseNotesDocument = {
  source: ReleaseNotesTabSource;
  markdown: string;
};

export function releaseNotesTitle(version: string): string {
  return `What's new in MonoCode ${version}`;
}

export function releaseNotesForVersion(
  version: string,
  changelog: string = bundledChangelog,
): ReleaseNotesDocument | null {
  const normalized = version.trim();
  if (!normalized || normalized === "Unreleased") return null;

  const escapedVersion = normalized.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const heading = new RegExp(
    `^## \\[${escapedVersion}\\](?: - \\d{4}-\\d{2}-\\d{2})?\\r?$`,
    "gm",
  );
  const match = heading.exec(changelog);
  if (!match) return null;

  const nextHeading = /^## /gm;
  nextHeading.lastIndex = match.index + match[0].length;
  const next = nextHeading.exec(changelog);
  const markdown = changelog.slice(match.index, next?.index).trimEnd();

  return {
    source: { version: normalized },
    markdown,
  };
}

export function releaseNotesMarkdown(
  source: ReleaseNotesTabSource,
  changelog: string = bundledChangelog,
): string | null {
  return releaseNotesForVersion(source.version, changelog)?.markdown ?? null;
}
