import bundledChangelog from "../../CHANGELOG.md?raw";
import { t } from "./i18n";

export type ReleaseNotesTabSource = {
  version: string;
};

export type ReleaseNotesDocument = {
  source: ReleaseNotesTabSource;
  markdown: string;
};

export function releaseNotesTitle(version: string): string {
  return `${t("What's new in MonoCode")} ${version}`;
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

export type ReleaseNotesPresentation = {
  version: string;
  date: string | null;
  markdown: string;
};

/** Changelog body for the What's new modal: version heading lives in the chrome. */
export function presentReleaseNotes(
  version: string,
  changelog: string = bundledChangelog,
): ReleaseNotesPresentation | null {
  const release = releaseNotesForVersion(version, changelog);
  if (!release) return null;

  const escapedVersion = release.source.version.replace(
    /[.*+?^${}()|[\]\\]/g,
    "\\$&",
  );
  const heading = new RegExp(
    `^## \\[${escapedVersion}\\](?: - (\\d{4}-\\d{2}-\\d{2}))?\\r?\\n*`,
  );
  const match = heading.exec(release.markdown);

  return {
    version: release.source.version,
    date: match?.[1] ?? null,
    markdown: match
      ? release.markdown.slice(match[0].length).trimStart()
      : release.markdown,
  };
}

const MONTHS = [
  t("Jan"),
  t("Feb"),
  t("Mar"),
  t("Apr"),
  t("May"),
  t("Jun"),
  t("Jul"),
  t("Aug"),
  t("Sep"),
  t("Oct"),
  t("Nov"),
  t("Dec"),
] as const;

export function formatReleaseDate(iso: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (!match) return iso;
  const month = MONTHS[Number(match[2]) - 1];
  if (!month) return iso;
  return `${Number(match[3])} ${month} ${match[1]}`;
}
