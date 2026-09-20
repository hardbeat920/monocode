import { limitSection, parseJsonObject, stringField } from "../../../shared/lib/jsonText";

export type CommitMessage = {
  subject: string;
  body: string;
};

export type PrContent = {
  title: string;
  body: string;
};

export function buildCommitMessagePrompt(input: {
  branch: string | null;
  stagedSummary: string;
  stagedPatch: string;
  includeBranch?: boolean;
}): string {
  const wantsBranch = input.includeBranch === true;
  return [
    "You write concise git commit messages.",
    wantsBranch
      ? "Return a JSON object with keys: subject, body, branch."
      : "Return a JSON object with keys: subject, body.",
    "Do not call tools. Reply with JSON only.",
    "Rules:",
    "- subject must be imperative, <= 72 chars, and no trailing period",
    "- body can be empty string or short bullet points",
    ...(wantsBranch
      ? ["- branch must be a short semantic git branch fragment for this change"]
      : []),
    "- capture the primary user-visible or developer-visible change",
    "",
    `Branch: ${input.branch ?? "(detached)"}`,
    "",
    "Staged files:",
    limitSection(input.stagedSummary, 6_000),
    "",
    "Staged patch:",
    limitSection(input.stagedPatch, 40_000),
  ].join("\n");
}

export function buildPrContentPrompt(input: {
  baseBranch: string;
  headBranch: string;
  commitSummary: string;
  diffSummary: string;
  diffPatch: string;
}): string {
  return [
    "You write source control change request content.",
    "Return a JSON object with keys: title, body.",
    "Do not call tools. Reply with JSON only.",
    "Rules:",
    "- title should be concise and specific",
    "- body must be markdown and include headings '## Summary' and '## Testing'",
    "- under Summary, provide short bullet points",
    "- under Testing, include bullet points with concrete checks or 'Not run' where appropriate",
    "",
    `Base branch: ${input.baseBranch}`,
    `Head branch: ${input.headBranch}`,
    "",
    "Commits:",
    limitSection(input.commitSummary, 12_000),
    "",
    "Diff stat:",
    limitSection(input.diffSummary, 12_000),
    "",
    "Diff patch:",
    limitSection(input.diffPatch, 40_000),
  ].join("\n");
}

export function buildBranchNamePrompt(message: string): string {
  return [
    "You generate concise git branch names.",
    "Return a JSON object with key: branch.",
    "Do not call tools. Reply with JSON only.",
    "Rules:",
    "- Branch should describe the requested work from the user message.",
    "- Keep it short and specific (2-6 words).",
    "- Use plain words only, no issue prefixes and no punctuation-heavy text.",
    "",
    "User message:",
    limitSection(message, 8_000),
  ].join("\n");
}

export function parseCommitMessage(raw: string): CommitMessage | null {
  const rec = parseJsonObject(raw);
  if (!rec) return null;
  const subject = sanitizeCommitSubject(
    stringField(rec, "subject") ||
      stringField(rec, "title") ||
      stringField(rec, "message"),
  );
  if (!subject) return null;
  return { subject, body: commitBody(rec) };
}

function commitBody(rec: Record<string, unknown>): string {
  const value = rec.body;
  if (typeof value === "string") return value.trim();
  if (Array.isArray(value)) {
    return value
      .flatMap((item) => (typeof item === "string" ? [item] : []))
      .join("\n")
      .trim();
  }
  return "";
}

export function formatCommitMessage(message: CommitMessage): string {
  return message.body ? `${message.subject}\n\n${message.body}` : message.subject;
}

export function parsePrContent(raw: string): PrContent | null {
  const rec = parseJsonObject(raw);
  if (!rec) return null;
  const title = sanitizePrTitle(stringField(rec, "title"));
  const body = stringField(rec, "body").trim();
  if (!title) return null;
  return { title, body };
}

export function parseBranchName(raw: string): string | null {
  const rec = parseJsonObject(raw);
  const branch = sanitizeBranchFragment(
    rec ? stringField(rec, "branch") : raw,
  );
  return branch || null;
}

export function sanitizeCommitSubject(raw: string): string {
  const singleLine = raw.trim().split(/\r?\n/g)[0]?.trim() ?? "";
  const withoutTrailingPeriod = singleLine.replace(/[.]+$/g, "").trim();
  if (!withoutTrailingPeriod) return "";
  if (withoutTrailingPeriod.length <= 72) return withoutTrailingPeriod;
  return withoutTrailingPeriod.slice(0, 72).trimEnd();
}

export function sanitizePrTitle(raw: string): string {
  return raw.trim().split(/\r?\n/g)[0]?.trim() ?? "";
}

export function sanitizeBranchFragment(raw: string): string {
  const normalized = raw
    .trim()
    .toLowerCase()
    .replace(/['"`]/g, "")
    .replace(/^[./\s_-]+|[./\s_-]+$/g, "");
  const fragment = normalized
    .replace(/[^a-z0-9/_-]+/g, "-")
    .replace(/\/+/g, "/")
    .replace(/-+/g, "-")
    .replace(/^[./_-]+|[./_-]+$/g, "")
    .slice(0, 64)
    .replace(/[./_-]+$/g, "");
  return fragment;
}
