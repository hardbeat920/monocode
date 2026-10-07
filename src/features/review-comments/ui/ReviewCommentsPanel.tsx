import { useSyncExternalStore } from "react";
import { ArrowUp, MessageSquarePlus, Trash2 } from "../../../shared/ui/icons";
import { requestAddToChat } from "../../sessions/model/quoteDraft";
import {
  clearReviewComments,
  formatReviewComments,
  removeReviewComment,
  subscribeReviewComments,
  reviewCommentsSnapshot,
  updateReviewComment,
} from "../model/reviewComments";

export function ReviewCommentsPanel({
  onAddFileComment,
}: {
  onAddFileComment: (anchor: DOMRect) => void;
}) {
  const comments = useSyncExternalStore(
    subscribeReviewComments,
    reviewCommentsSnapshot,
    reviewCommentsSnapshot,
  );

  const submit = () => {
    const text = formatReviewComments(comments);
    if (!text) return;
    requestAddToChat(text, "plain");
    clearReviewComments();
  };

  return (
    <div className="shrink-0 border-b border-stroke bg-background-base/80 px-2 py-1.5">
      <div className="flex items-center justify-end gap-1">
        <button
          type="button"
          onClick={(event) =>
            onAddFileComment(event.currentTarget.getBoundingClientRect())
          }
          className="inline-flex items-center gap-1.5 rounded px-2 py-1 text-xs text-content/70 hover:bg-content/10"
        >
          <MessageSquarePlus className="size-3.5" />
          Add comment
        </button>
        {comments.length > 0 ? (
          <button
            type="button"
            onClick={submit}
            className="inline-flex items-center gap-1.5 rounded bg-content px-2 py-1 text-xs font-medium text-background-base hover:bg-content/80"
          >
            <ArrowUp className="size-3.5" />
            Submit comments ({comments.length})
          </button>
        ) : null}
      </div>
      {comments.length > 0 ? (
        <div className="mt-1 max-h-48 space-y-2 overflow-auto px-1 py-1">
          {comments.map((comment) => (
            <div key={comment.id} className="rounded border border-content/10 p-2">
              <div className="mb-1 flex items-center justify-between gap-2">
                <span className="truncate font-mono text-[10px] text-content/55">
                  {comment.path}
                  {comment.startLine === 0
                    ? ""
                    : `:${comment.startLine}${comment.endLine === comment.startLine ? "" : `-${comment.endLine}`}`}
                </span>
                <button
                  type="button"
                  onClick={() => removeReviewComment(comment.id)}
                  aria-label="Remove review comment"
                  className="text-content/45 hover:text-content"
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
              <textarea
                value={comment.body}
                onChange={(event) =>
                  updateReviewComment(comment.id, event.target.value)
                }
                rows={2}
                aria-label={`Review comment for ${comment.path}`}
                className="w-full resize-y bg-transparent text-xs text-content outline-none"
              />
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
