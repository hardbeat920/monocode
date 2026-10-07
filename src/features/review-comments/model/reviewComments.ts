export type ReviewComment = {
  id: string;
  path: string;
  startLine: number;
  endLine: number;
  snippet: string;
  body: string;
  deleted?: boolean;
};

let comments: ReviewComment[] = [];
const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((listener) => listener());
}

export function subscribeReviewComments(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function reviewCommentsSnapshot() {
  return comments;
}

export function addReviewComment(
  input: Omit<ReviewComment, "id">,
): ReviewComment {
  const comment = { ...input, id: crypto.randomUUID() };
  comments = [...comments, comment];
  notify();
  return comment;
}

export function updateReviewComment(id: string, body: string) {
  const normalized = body.trim();
  if (!normalized) return removeReviewComment(id);
  comments = comments.map((comment) =>
    comment.id === id ? { ...comment, body: normalized } : comment,
  );
  notify();
}

export function removeReviewComment(id: string) {
  const next = comments.filter((comment) => comment.id !== id);
  if (next.length === comments.length) return;
  comments = next;
  notify();
}

export function clearReviewComments() {
  if (comments.length === 0) return;
  comments = [];
  notify();
}

export function formatReviewComments(items = comments) {
  if (items.length === 0) return "";
  return [
    "Please address these review comments:",
    "",
    ...items.flatMap((comment, index) => {
      const lines =
        comment.startLine === 0
          ? "file"
          : comment.startLine === comment.endLine
          ? `${comment.startLine}`
          : `${comment.startLine}-${comment.endLine}`;
      const deleted = comment.deleted ? " (deleted line)" : "";
      const snippet = truncateSnippet(comment.snippet);
      return [
        `${index + 1}. \`${escapeTicks(comment.path)}\`${lines === "file" ? " (file)" : `:${lines}`}${deleted}`,
        `   ${comment.body.replace(/\n/g, "\n   ")}`,
        ...(snippet ? ["", "   ```", ...snippet.split("\n").map((line) => `   ${line}`), "   ```"] : []),
        "",
      ];
    }),
  ]
    .join("\n")
    .trim();
}

function truncateSnippet(text: string) {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const limited = lines.slice(0, 20).join("\n").slice(0, 2_000);
  return limited === text ? limited : `${limited}\n…`;
}

function escapeTicks(value: string) {
  return value.replace(/`/g, "\\`");
}
