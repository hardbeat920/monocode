import { openUrl } from "@tauri-apps/plugin-opener";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { LoaderCircle, X } from "../chrome/icons";
import {
  formatRelativeTime,
  githubReviewStateLabel,
  inboxPersonAvatarUrl,
  type InboxProvider,
} from "../lib/githubTasks";
import { MOD } from "../lib/platform";
import { AgentMarkdown } from "./AgentMarkdown";

export type InboxReplyTarget = {
  id: string;
  author: string;
  threadId: string;
};

type InboxComment = {
  id: string;
  kind: string;
  author: string;
  authorAvatarUrl?: string;
  body: string;
  createdAt: string;
  url: string;
  state: string;
  path: string;
  line: number | null;
  resolved: boolean;
  threadId?: string;
  replies: InboxComment[];
};

type InboxThread = {
  comments: InboxComment[];
  truncated: boolean;
};

type Props = {
  thread: InboxThread | null;
  loading: boolean;
  error: string | null;
  cwd: string;
  provider: InboxProvider;
  replyMode?: "thread" | "parent";
  onReply?: (target: InboxReplyTarget) => void;
};

export function InboxComments({
  thread,
  loading,
  error,
  cwd,
  provider,
  replyMode,
  onReply,
}: Props) {
  if (thread && thread.comments.length === 0 && !thread.truncated) {
    if (loading) return <CommentsPending />;
    return null;
  }
  if (!thread) {
    if (error) {
      return <p className="text-[12px] text-content/45">{error}</p>;
    }
    if (loading) return <CommentsPending />;
    return null;
  }

  const count = thread.comments.reduce(
    (total, comment) => total + 1 + comment.replies.length,
    0,
  );
  const label = count === 1 ? "1 comment" : `${count} comments`;
  const moreOn = provider === "linear" ? "Linear" : "GitHub";

  return (
    <section className="flex flex-col gap-3 border-t border-content/10 pt-5">
      <div className="flex items-center gap-2 text-[12px] text-content/50">
        <h2 className="text-content/70">{label}</h2>
        {thread.truncated ? (
          <span>Latest comments · more on {moreOn}</span>
        ) : null}
        {loading ? (
          <LoaderCircle
            className="size-3 animate-spin text-content/35"
            strokeWidth={1.75}
          />
        ) : null}
      </div>
      {error ? <p className="text-[12px] text-content/45">{error}</p> : null}
      <ol className="flex flex-col gap-2">
        {thread.comments.map((comment) => (
          <li key={comment.id}>
            <InboxComment
              comment={comment}
              cwd={cwd}
              provider={provider}
              replyMode={replyMode}
              onReply={onReply}
            />
          </li>
        ))}
      </ol>
    </section>
  );
}

export function InboxCommentForm({
  replyTo,
  posting,
  error,
  onCancelReply,
  onSubmit,
}: {
  replyTo: InboxReplyTarget | null;
  posting: boolean;
  error: string | null;
  onCancelReply: () => void;
  onSubmit: (body: string) => Promise<void>;
}) {
  const [draft, setDraft] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);
  const canPost = draft.trim().length > 0 && !posting;

  useEffect(() => {
    if (!replyTo) return;
    field.current?.focus();
  }, [replyTo]);

  useEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [draft]);

  const submit = async () => {
    const body = draft.trim();
    if (!body || posting) return;
    try {
      await onSubmit(body);
      setDraft("");
    } catch {
      // Parent keeps the error next to the button.
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) return;
    event.preventDefault();
    void submit();
  };

  const onFormSubmit = (event: FormEvent) => {
    event.preventDefault();
    void submit();
  };

  return (
    <form
      onSubmit={onFormSubmit}
      className="flex flex-col gap-2 border-t border-content/10 pt-5"
    >
      {replyTo ? (
        <div className="flex items-center gap-2 text-[12px] text-content/50">
          <span className="min-w-0 truncate">
            Replying to {replyTo.author || "comment"}
          </span>
          <button
            type="button"
            title="Cancel reply"
            aria-label="Cancel reply"
            onClick={onCancelReply}
            className="grid size-5 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
          >
            <X className="size-3" strokeWidth={1.75} />
          </button>
        </div>
      ) : null}
      <div className="rounded-md border border-content/10 bg-content/5 focus-within:border-content/20">
        <textarea
          ref={field}
          rows={2}
          value={draft}
          disabled={posting}
          placeholder={
            replyTo ? `Write a reply (${MOD}↩)` : `Leave a comment (${MOD}↩)`
          }
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={onKeyDown}
          className="max-h-40 w-full resize-none overflow-y-auto bg-transparent px-3 py-2 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 disabled:opacity-40"
        />
        <div className="flex items-center justify-end px-2 pb-2">
          <button
            type="submit"
            disabled={!canPost}
            className="inline-flex h-7 items-center rounded-md bg-content px-3 text-[12px] text-background-base hover:bg-content/80 disabled:cursor-default disabled:opacity-40"
          >
            {posting ? "Posting..." : replyTo ? "Reply" : "Comment"}
          </button>
        </div>
      </div>
      {error ? (
        <p className="text-[12px] text-red-400/90">{error}</p>
      ) : null}
    </form>
  );
}

function CommentsPending() {
  return (
    <div className="flex items-center gap-2 border-t border-content/10 pt-5 text-[12px] text-content/45">
      <LoaderCircle className="size-3.5 animate-spin" strokeWidth={1.75} />
      Loading comments
    </div>
  );
}

function InboxComment({
  comment,
  cwd,
  provider,
  nested = false,
  replyMode,
  onReply,
}: {
  comment: InboxComment;
  cwd: string;
  provider: InboxProvider;
  nested?: boolean;
  replyMode?: "thread" | "parent";
  onReply?: (target: InboxReplyTarget) => void;
}) {
  const time = formatRelativeTime(comment.createdAt);
  const review = githubReviewStateLabel(comment.state);
  const location = commentLocation(comment);
  const meta = [
    review,
    location,
    comment.resolved ? "Resolved" : "",
    time,
  ].filter((part) => part.length > 0);
  const hasBody = comment.body.trim().length > 0;
  const hasReplies = !nested && comment.replies.length > 0;
  const canReply =
    onReply != null &&
    (replyMode === "parent" ||
      (replyMode === "thread" && (comment.threadId ?? "").trim().length > 0));

  const inner = (
    <>
      <header
        className={`flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-content/50 ${
          nested ? "" : "px-3 py-2"
        } ${!nested && (hasBody || hasReplies) ? "border-b border-content/10" : ""}`}
      >
        <InboxCommentPerson
          name={comment.author || "ghost"}
          avatarUrl={inboxPersonAvatarUrl(
            provider,
            comment.author,
            comment.authorAvatarUrl,
          )}
        />
        {meta.map((part, index) => (
          <span
            key={`${part}-${index}`}
            className="flex min-w-0 items-center gap-2"
          >
            <span aria-hidden>·</span>
            {comment.url && part === time ? (
              <button
                type="button"
                title={
                  provider === "linear" ? "Open in Linear" : "Open on GitHub"
                }
                onClick={() => void openUrl(comment.url)}
                className="hover:text-content"
              >
                {part}
              </button>
            ) : (
              <span
                className={
                  comment.state === "APPROVED"
                    ? "text-emerald-400/90"
                    : comment.state === "CHANGES_REQUESTED"
                      ? "text-rose-400/90"
                      : comment.resolved && part === "Resolved"
                        ? "text-emerald-400/80"
                        : "min-w-0 truncate"
                }
              >
                {part}
              </span>
            )}
          </span>
        ))}
        {canReply ? (
          <span className="flex items-center gap-2">
            <span aria-hidden>·</span>
            <button
              type="button"
              onClick={() =>
                onReply({
                  id: comment.id,
                  author: comment.author || "ghost",
                  threadId: (comment.threadId ?? "").trim(),
                })
              }
              className="hover:text-content"
            >
              Reply
            </button>
          </span>
        ) : null}
      </header>
      {hasBody ? (
        <div className={nested ? "mt-2" : "px-3 py-2.5"}>
          <AgentMarkdown
            className="inbox-comment-md"
            text={comment.body}
            cwd={cwd}
            allowRemoteMedia
          />
        </div>
      ) : null}
      {hasReplies ? (
        <div className="border-t border-content/10 px-3">
          {comment.replies.map((reply, index) => (
            <div
              key={reply.id}
              className={`py-2.5 ${
                index > 0 ? "border-t border-content/10" : ""
              }`}
            >
              <InboxComment
                comment={reply}
                cwd={cwd}
                provider={provider}
                nested
                replyMode={replyMode}
                onReply={onReply}
              />
            </div>
          ))}
        </div>
      ) : null}
    </>
  );

  if (nested) return <article>{inner}</article>;
  return (
    <article className="overflow-hidden rounded-md border border-content/10 bg-content/5">
      {inner}
    </article>
  );
}

function commentLocation(comment: InboxComment): string {
  const path = comment.path.trim();
  if (!path) return "";
  if (comment.line && comment.line > 0) return `${path}:${comment.line}`;
  return path;
}

function InboxCommentPerson({
  name,
  avatarUrl,
}: {
  name: string;
  avatarUrl: string;
}) {
  const [failed, setFailed] = useState(!avatarUrl);
  const initial = name.trim().charAt(0).toUpperCase() || "?";

  useEffect(() => {
    setFailed(!avatarUrl);
  }, [avatarUrl]);

  return (
    <span className="inline-flex min-w-0 items-center gap-1.5">
      {avatarUrl && !failed ? (
        <img
          src={avatarUrl}
          alt=""
          width={20}
          height={20}
          referrerPolicy="no-referrer"
          draggable={false}
          onError={() => setFailed(true)}
          className="size-5 shrink-0 rounded-full bg-content/10 object-cover"
        />
      ) : (
        <span
          aria-hidden
          className="grid size-5 shrink-0 place-items-center rounded-full bg-content/12 text-[10px] font-medium text-content/55"
        >
          {initial}
        </span>
      )}
      <span className="min-w-0 truncate font-medium text-content">{name}</span>
    </span>
  );
}
