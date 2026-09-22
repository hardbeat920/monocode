import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import {
  ArrowUp,
  MessageSquarePlus,
  RefreshCw,
  Trash2,
  X,
} from "../../../shared/ui/icons";

import { Popover } from "../../../shared/ui/Popover";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { AgentMarkdown } from "./AgentMarkdown";
import { ModelPicker } from "./ModelPicker";
import { HarnessIcon as ProviderIcon } from "./HarnessIcon";
import type { BtwThread, BtwMessage, HarnessId } from "../model/session";

type Props = {
  threads?: BtwThread[];
  visible?: boolean;
  cwd?: string;
  model?: string;
  onSubmit: (
    threadId: string,
    messageId: string,
    text: string,
    model?: string,
  ) => void;
  onRetry: (threadId: string) => void;
  onDelete?: (threadId: string) => void;
  onModelChange?: (threadId: string, model: string) => void;
};

function shortQuestion(thread: BtwThread): string {
  const question = thread.messages.find(
    (message) => message.role === "user",
  )?.text;
  const compact = question?.replace(/\s+/g, " ").trim() || "Side question";
  return compact.length > 72 ? `${compact.slice(0, 69)}…` : compact;
}

function statusLabel(thread: BtwThread): string {
  if (thread.status === "running") return "Working";
  if (thread.status === "error") return "Needs retry";
  return "Ready";
}

function statusClass(status: BtwThread["status"]): string {
  if (status === "running") return "bg-amber-300";
  if (status === "error") return "bg-red-300";
  return "bg-emerald-300";
}

const BTW_ALLOWED_HARNESSES = ["codex"] as const;

function messageMeta(role: BtwMessage["role"]) {
  return role === "user" ? (
    <div className="btw-message-meta btw-message-meta-user">
      <span>YOU</span>
      <span className="btw-message-rule" aria-hidden />
    </div>
  ) : (
    <div className="btw-message-meta">
      <ProviderIcon harness="codex" className="size-3.5 shrink-0" />
      <span>CODEX</span>
    </div>
  );
}

export function ByTheWayPopover({
  threads = [],
  visible = true,
  cwd,
  model = "",
  onSubmit,
  onRetry,
  onDelete,
  onModelChange,
}: Props) {
  const [openThreadId, setOpenThreadId] = useState<string | null>(null);
  const [draftText, setDraftText] = useState("");
  const [draftModel, setDraftModel] = useState<string | null>(null);
  const [optimistic, setOptimistic] = useState<{
    threadId: string;
    message: BtwMessage;
  } | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const persisted = useMemo(
    () => threads.find((thread) => thread.id === openThreadId),
    [openThreadId, threads],
  );
  const optimisticForThread =
    optimistic?.threadId === openThreadId ? optimistic.message : undefined;
  const messages = persisted?.messages ?? [];
  const displayedMessages =
    optimisticForThread &&
    !messages.some((message) => message.id === optimisticForThread.id)
      ? [...messages, optimisticForThread]
      : messages;
  const running = persisted?.status === "running" || !!optimisticForThread;
  const selectedModel = persisted?.model ?? draftModel ?? model;
  const open = openThreadId != null;

  const close = () => {
    setOpenThreadId(null);
    setDraftText("");
    setDraftModel(null);
    triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!visible && open) close();
  }, [visible, open]);

  useEffect(() => {
    if (!open || !composerRef.current) return;
    composerRef.current.focus();
  }, [openThreadId, open]);

  useEffect(() => {
    if (
      optimistic &&
      threads.some(
        (thread) =>
          thread.id === optimistic.threadId &&
          thread.messages.some(
            (message) => message.id === optimistic.message.id,
          ),
      )
    ) {
      setOptimistic(null);
    }
  }, [optimistic, threads]);

  const openNew = (trigger?: HTMLButtonElement) => {
    if (trigger) triggerRef.current = trigger;
    setOptimistic(null);
    setDraftText("");
    setDraftModel(null);
    setOpenThreadId(crypto.randomUUID());
  };

  const openNewFromTrigger = (event: MouseEvent<HTMLButtonElement>) => {
    openNew(event.currentTarget);
  };

  const openExisting = (event: MouseEvent<HTMLButtonElement>, id: string) => {
    triggerRef.current = event.currentTarget;
    setOptimistic(null);
    setDraftText("");
    setDraftModel(null);
    setOpenThreadId(id);
  };

  const submit = () => {
    const text = draftText.trim();
    if (!text || running) return;
    const threadId = openThreadId ?? crypto.randomUUID();
    const messageId = crypto.randomUUID();
    setOptimistic({
      threadId,
      message: { id: messageId, role: "user", text, createdAt: Date.now() },
    });
    setOpenThreadId(threadId);
    setDraftText("");
    onSubmit(threadId, messageId, text, selectedModel || undefined);
  };

  const onComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    submit();
  };

  const handleModelChange = (nextHarness: HarnessId, nextModel: string) => {
    if (nextHarness !== "codex") return;
    setDraftModel(nextModel);
    if (persisted && openThreadId) onModelChange?.(openThreadId, nextModel);
  };
  const deleteThread = () => {
    if (!persisted || !openThreadId) return;
    onDelete?.(openThreadId);
    close();
  };

  return (
    <span className="contents">
      <button
        type="button"
        aria-label="Ask a BTW question"
        title="Ask a BTW question"
        ref={triggerRef}
        onClick={openNewFromTrigger}
        className="shrink-0 rounded-md p-1.5 text-content/35 transition-colors hover:bg-content/8 hover:text-content/80 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
      >
        <MessageSquarePlus className="size-3.5" strokeWidth={1.75} />
      </button>
      <span className="min-w-0 flex-1 overflow-x-auto scrollbar-none">
        <span className="inline-flex w-max max-w-none items-center gap-1.5">
          {threads.map((thread) => (
            <button
              type="button"
              key={thread.id}
              aria-label={`${statusLabel(thread)}: ${shortQuestion(thread)}`}
              title={shortQuestion(thread)}
              onClick={(event) => openExisting(event, thread.id)}
              className="btw-thread-chip inline-flex min-h-6 max-w-[17rem] items-center gap-1.5 rounded-md px-2 text-left text-content/55 transition-colors hover:bg-content/8 hover:text-content/90 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
            >
              <span
                className={`size-1.5 shrink-0 rounded-full ${statusClass(thread.status)}`}
                aria-hidden
              />
              <span className="truncate">{shortQuestion(thread)}</span>
            </button>
          ))}
        </span>
      </span>
      {openThreadId && triggerRef.current ? (
        <Popover
          anchor={triggerRef}
          side="right"
          align="start"
          gap={10}
          padding={12}
          width={420}
          maxHeight={680}
          data-btw-popover
          role="dialog"
          aria-label="By-the-way conversation"
          onDismiss={close}
          ignore="[data-model-picker]"
          className="btw-popover-surface flex min-h-0 flex-col font-sans text-sm text-content"
        >
          <div className="flex min-h-12 shrink-0 items-center justify-between gap-4 border-b border-content/10 px-4 py-2.5">
            <div className="min-w-0 font-medium tracking-[-0.01em]">BTW</div>
            <div className="flex shrink-0 items-center gap-1">
              {persisted && onDelete ? (
                <button
                  type="button"
                  aria-label="Delete BTW conversation"
                  title="Delete BTW conversation"
                  onClick={deleteThread}
                  className="grid size-7 place-items-center rounded-md text-content/35 transition-colors hover:bg-red-400/10 hover:text-red-200 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-red-300/70"
                >
                  <Trash2 className="size-3.5" strokeWidth={1.75} />
                </button>
              ) : null}
              <button
                type="button"
                aria-label="Close by-the-way conversation"
                onClick={close}
                className="grid size-7 shrink-0 place-items-center rounded-md text-content/45 transition-colors hover:bg-content/8 hover:text-content focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
              >
                <X className="size-4" strokeWidth={1.75} />
              </button>
            </div>
          </div>
          <div className="scrollbar-none min-h-0 flex-1 overflow-y-auto px-4 py-4">
            {displayedMessages.length === 0 ? (
              <div className="btw-empty-state">
                <MessageSquarePlus
                  className="size-4 text-content/35"
                  strokeWidth={1.5}
                />
                <p>Ask about this turn.</p>
                <span>
                  Side questions stay separate from the main conversation.
                </span>
              </div>
            ) : (
              <div className="space-y-5">
                {displayedMessages.map((message) => (
                  <div
                    key={message.id}
                    className={
                      message.role === "user"
                        ? "btw-message btw-message-user ml-auto max-w-[92%]"
                        : "btw-message btw-message-assistant max-w-[96%]"
                    }
                  >
                    {messageMeta(message.role)}
                    <AgentMarkdown
                      text={message.text}
                      cwd={cwd}
                      className="mt-1.5 text-[13px] leading-5"
                    />
                  </div>
                ))}
                {running ? (
                  <div className="btw-message btw-message-assistant max-w-[96%]">
                    <div className="btw-message-meta">
                      <ProviderIcon
                        harness="codex"
                        className="size-3.5 shrink-0"
                      />
                      <span>CODEX</span>
                      <span className="btw-message-rule" aria-hidden />
                      <span className="normal-case tracking-normal text-content/45">
                        thinking
                      </span>
                    </div>
                    <Shimmer
                      duration={1.6}
                      className="mt-1.5 text-[13px] leading-5 text-content/55"
                    >
                      Working through a separate thread…
                    </Shimmer>
                  </div>
                ) : null}
              </div>
            )}
            {persisted?.status === "error" ? (
              <div className="btw-error mt-5" role="alert">
                <div className="min-w-0">
                  <div className="text-[10px] font-medium uppercase tracking-[0.08em] text-red-200/70">
                    Couldn’t finish
                  </div>
                  <div className="mt-1 text-[12px] leading-4.5 text-red-100/75">
                    {persisted.error ||
                      "Codex could not answer this side question."}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => onRetry(persisted.id)}
                  className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-[11px] font-medium text-red-100/80 transition-colors hover:bg-red-200/10 hover:text-red-50 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-red-200/60"
                >
                  <RefreshCw className="size-3" strokeWidth={1.75} />
                  Retry
                </button>
              </div>
            ) : null}
          </div>

          <div className="shrink-0 border-t border-content/10 px-3.5 py-3">
            <textarea
              ref={composerRef}
              value={draftText}
              rows={2}
              placeholder={
                running ? "Codex is thinking…" : "Ask a side question…"
              }
              aria-label="By-the-way question"
              disabled={running}
              onChange={(event) => setDraftText(event.target.value)}
              onKeyDown={onComposerKeyDown}
              className="btw-composer-field block w-full resize-none rounded-lg px-3 py-2.5 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 disabled:cursor-wait disabled:opacity-55"
            />
            <div className="mt-2 flex items-center justify-between gap-2">
              {selectedModel ? (
                <ModelPicker
                  harness="codex"
                  model={selectedModel}
                  values={{}}
                  hideSettings
                  allowedHarnesses={BTW_ALLOWED_HARNESSES}
                  onChange={handleModelChange}
                  onSettingsChange={() => undefined}
                  onClose={() => composerRef.current?.focus()}
                />
              ) : null}
              <div className="flex items-center gap-2">
                <span className="hidden text-[10px] text-content/30 sm:inline">
                  Enter to send
                </span>
                <button
                  type="button"
                  aria-label="Send by-the-way question"
                  disabled={!draftText.trim() || running}
                  onClick={submit}
                  className="primary-action inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[11px] font-medium transition-transform active:scale-[0.97] disabled:cursor-default"
                >
                  <ArrowUp className="size-3.5" strokeWidth={2.25} />
                  Send
                </button>
              </div>
            </div>
            {running ? (
              <div
                className="mt-2 flex items-center gap-1.5 text-[11px] text-content/40"
                role="status"
              >
                <span
                  className="size-1.5 rounded-full bg-amber-300/80"
                  aria-hidden
                />
                Codex is answering separately; the main turn is unchanged.
              </div>
            ) : null}
          </div>
        </Popover>
      ) : null}
    </span>
  );
}
