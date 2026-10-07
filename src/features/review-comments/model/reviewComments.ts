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

/** Keeps review comment locations aligned with edits to their source file. */
export function remapReviewCommentLines(
  path: string,
  remapLine: (line: number) => number,
) {
  let changed = false;
  comments = comments.map((comment) => {
    if (comment.path !== path || comment.startLine === 0) return comment;
    const startLine = remapLine(comment.startLine);
    const endLine = remapLine(comment.endLine);
    if (startLine === comment.startLine && endLine === comment.endLine) {
      return comment;
    }
    changed = true;
    return { ...comment, startLine, endLine };
  });
  if (changed) notify();
}

/** Marks comments as stale when an external full-file reload has no line map. */
export function markReviewCommentsStale(path: string) {
  let changed = false;
  comments = comments.map((comment) => {
    if (comment.path !== path || comment.deleted) return comment;
    changed = true;
    return { ...comment, deleted: true };
  });
  if (changed) notify();
}

export function formatReviewComments(items = comments) {
  if (items.length === 0) return "";
  return items
    .map((comment) => {
      const reference =
        comment.startLine === 0
          ? `@${comment.path} (file)`
          : `@${comment.path} (${comment.startLine === comment.endLine ? `line ${comment.startLine}` : `lines ${comment.startLine}-${comment.endLine}`})${comment.deleted ? " (deleted)" : ""}`;
      return `${reference}\n${comment.body}`;
    })
    .join("\n\n");
}
