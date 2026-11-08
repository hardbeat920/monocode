/**
 * Detects when a link in agent output points at a GitHub pull request, commit,
 * or issue so the transcript can decorate it with a GitHub badge. The badge is
 * purely presentational: clicking it still opens the original href.
 */

export type GithubRefKind = "pr" | "commit" | "issue";

export type GithubRef = {
  kind: GithubRefKind;
  /** `owner/repo`, handy for a tooltip. */
  repo: string;
  /** Short label shown in the badge, e.g. `#1343` or `a1b2c3d`. */
  label: string;
};

/**
 * Clamps a title to at most `maxWords` words, appending an ellipsis when it is
 * trimmed. Keeps the hover-revealed badge title from growing without bound.
 */
export function truncateWords(text: string, maxWords: number): string {
  const words = text.trim().split(/\s+/);
  if (words.length <= maxWords) return words.join(" ");
  return `${words.slice(0, maxWords).join(" ")}…`;
}

const HOSTS = new Set(["github.com", "www.github.com"]);

/** Owner/repo segments may not contain slashes; keep the match tight. */
const SEGMENT = "[^/]+";

function shortSha(sha: string): string {
  return /^[0-9a-f]{7,40}$/i.test(sha) ? sha.slice(0, 7) : sha;
}

/**
 * Returns badge metadata when `href` is a GitHub PR, commit, or issue URL, or
 * `null` for anything else (plain web links keep their normal rendering).
 */
export function parseGithubRef(href: string | undefined | null): GithubRef | null {
  if (!href) return null;

  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return null;
  }

  if (url.protocol !== "https:" && url.protocol !== "http:") return null;
  if (!HOSTS.has(url.hostname.toLowerCase())) return null;

  const path = url.pathname.replace(/\/+$/, "");

  const pr = path.match(new RegExp(`^/(${SEGMENT})/(${SEGMENT})/pull/(\\d+)$`));
  if (pr) {
    return { kind: "pr", repo: `${pr[1]}/${pr[2]}`, label: `#${pr[3]}` };
  }

  const issue = path.match(
    new RegExp(`^/(${SEGMENT})/(${SEGMENT})/issues/(\\d+)$`),
  );
  if (issue) {
    return {
      kind: "issue",
      repo: `${issue[1]}/${issue[2]}`,
      label: `#${issue[3]}`,
    };
  }

  const commit = path.match(
    new RegExp(`^/(${SEGMENT})/(${SEGMENT})/commit/([0-9a-f]{7,40})$`, "i"),
  );
  if (commit) {
    return {
      kind: "commit",
      repo: `${commit[1]}/${commit[2]}`,
      label: shortSha(commit[3]),
    };
  }

  return null;
}
