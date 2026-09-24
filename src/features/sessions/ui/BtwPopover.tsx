import {
  Fragment,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
} from "react";
import {
  MessageSquarePlus,
  RefreshCw,
  Trash2,
  X,
} from "../../../shared/ui/icons";

import { Popover } from "../../../shared/ui/Popover";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { Composer } from "./Composer";
import { AgentMarkdown } from "./AgentMarkdown";
import { HarnessIcon as ProviderIcon } from "./HarnessIcon";
import { preferredModelSettings, resolveModel } from "../model/models";
import {
  DEFAULT_RUNTIME_MODE,
  HARNESS_LABEL,
  HARNESS_TITLE,
  type BtwMessage,
  type BtwThread,
  type HarnessId,
} from "../model/session";

export type BtwOpenRequest = {
  id: number;
  text: string;
};

type Props = {
  harness: HarnessId;
  threads?: BtwThread[];
  visible?: boolean;
  cwd?: string;
  model?: string;
  modelSettings?: Record<string, string>;
  openRequest?: BtwOpenRequest | null;
  onOpenRequestHandled?: (requestId: number) => void;
  onSubmit: (
    threadId: string,
    messageId: string,
    text: string,
    model?: string,
    modelSettings?: Record<string, string>,
  ) => void;
  onRetry: (threadId: string) => void;
  onDelete?: (threadId: string) => void;
  onModelChange?: (
    threadId: string,
    model: string,
    modelSettings: Record<string, string>,
  ) => void;
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

function messageMeta(role: BtwMessage["role"], harness: HarnessId) {
  if (role === "user") return null;
  return (
    <div className="btw-message-meta">
      <ProviderIcon harness={harness} className="size-3.5 shrink-0" />
      <span>{HARNESS_LABEL[harness].toUpperCase()}</span>
    </div>
  );
}

export function BtwPopover({
  harness,
  threads = [],
  visible = true,
  openRequest,
  onOpenRequestHandled,
  cwd,
  model = "",
  modelSettings = {},
  onSubmit,
  onRetry,
  onDelete,
  onModelChange,
}: Props) {
  const [openThreadId, setOpenThreadId] = useState<string | null>(null);
  const [draftText, setDraftText] = useState("");
  const [draftResetToken, setDraftResetToken] = useState(0);
  const [draftModel, setDraftModel] = useState<string | null>(null);
  const [draftModelSettings, setDraftModelSettings] = useState<Record<
    string,
    string
  > | null>(null);
  const [optimistic, setOptimistic] = useState<{
    threadId: string;
    message: BtwMessage;
  } | null>(null);
  const pendingSubmitRef = useRef<string | null>(null);
  const pendingEmptyFocusRef = useRef(false);
  const lastOpenRequestRef = useRef<number | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const btwComposerRef = useRef<HTMLDivElement | null>(null);
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
  const selectedModel = draftModel ?? persisted?.model ?? model;
  const selectedModelSettings = useMemo(() => {
    if (draftModelSettings) return draftModelSettings;
    if (persisted?.modelSettings) return persisted.modelSettings;
    return preferredModelSettings(
      resolveModel(harness, selectedModel),
      modelSettings,
    );
  }, [
    draftModelSettings,
    harness,
    modelSettings,
    persisted?.modelSettings,
    selectedModel,
  ]);
  const open = openThreadId != null;

  const close = () => {
    pendingSubmitRef.current = null;
    setOpenThreadId(null);
    setDraftText("");
    setDraftModel(null);
    setDraftModelSettings(null);
    triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!visible && open) close();
  }, [visible, open]);

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
    pendingSubmitRef.current = null;
    if (trigger) triggerRef.current = trigger;
    setOptimistic(null);
    setDraftText("");
    setDraftModel(null);
    setDraftModelSettings(null);
    setOpenThreadId(crypto.randomUUID());
  };

  const openNewFromTrigger = (event: MouseEvent<HTMLButtonElement>) => {
    openNew(event.currentTarget);
  };

  const openExisting = (event: MouseEvent<HTMLButtonElement>, id: string) => {
    pendingSubmitRef.current = null;
    triggerRef.current = event.currentTarget;
    setOptimistic(null);
    setDraftText("");
    setDraftModel(null);
    setDraftModelSettings(null);
    setOpenThreadId(id);
  };

  const submit = (value = draftText) => {
    const text = value.trim();
    if (!text || running) return;
    const threadId = openThreadId ?? crypto.randomUUID();
    const messageId = crypto.randomUUID();
    setOptimistic({
      threadId,
      message: { id: messageId, role: "user", text, createdAt: Date.now() },
    });
    setDraftResetToken((revision) => revision + 1);
    setOpenThreadId(threadId);
    setDraftText("");
    onSubmit(
      threadId,
      messageId,
      text,
      selectedModel || undefined,
      selectedModelSettings,
    );
  };

  useEffect(() => {
    if (!openRequest || lastOpenRequestRef.current === openRequest.id) return;
    lastOpenRequestRef.current = openRequest.id;
    const text = openRequest.text.trim();
    setOptimistic(null);
    setDraftModel(null);
    setDraftModelSettings(null);
    setDraftText(text);
    if (text) pendingSubmitRef.current = text;
    else pendingEmptyFocusRef.current = true;
    setOpenThreadId(crypto.randomUUID());
    onOpenRequestHandled?.(openRequest.id);
  }, [onOpenRequestHandled, openRequest]);

  useEffect(() => {
    if (!open || running || !pendingEmptyFocusRef.current) return;
    pendingEmptyFocusRef.current = false;
    const frame = requestAnimationFrame(() => {
      btwComposerRef.current?.querySelector("textarea")?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [open, openThreadId, running]);

  useEffect(() => {
    const text = pendingSubmitRef.current;
    if (!text || !openThreadId || draftText !== text || running) return;
    pendingSubmitRef.current = null;
    submit(text);
  }, [draftText, openThreadId, running]);

  const handleModelChange = (nextHarness: HarnessId, nextModel: string) => {
    if (nextHarness !== harness) return;
    const nextSettings = preferredModelSettings(
      resolveModel(nextHarness, nextModel),
      selectedModelSettings,
    );
    setDraftModel(nextModel);
    setDraftModelSettings(nextSettings);
    if (persisted && openThreadId) {
      onModelChange?.(openThreadId, nextModel, nextSettings);
    }
  };

  const handleSettingsChange = (nextSettings: Record<string, string>) => {
    setDraftModelSettings(nextSettings);
    if (persisted && openThreadId) {
      onModelChange?.(openThreadId, selectedModel, nextSettings);
    }
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
              className="btw-thread-chip inline-flex min-h-6 max-w-[17rem] items-center gap-1.5 rounded-md px-2 text-left text-xs text-content/55 transition-colors hover:bg-content/8 hover:text-content/90 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
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
                  <Fragment key={message.id}>
                    <div
                      className={
                        message.role === "user"
                          ? "user-message-bubble btw-message relative ml-auto w-fit max-w-[92%] rounded-full bg-content/10 px-3 py-2 font-sans text-content transition-[background-color] duration-200"
                          : "btw-message btw-message-assistant max-w-[96%]"
                      }
                    >
                      {messageMeta(message.role, harness)}
                      <AgentMarkdown
                        text={message.text}
                        cwd={cwd}
                        className={
                          message.role === "user"
                            ? "text-sm leading-5"
                            : "mt-1.5 text-[13px] leading-5"
                        }
                      />
                    </div>
                  </Fragment>
                ))}
                {running ? (
                  <div className="btw-message btw-message-assistant max-w-[96%]">
                    <div className="btw-message-meta">
                      <ProviderIcon
                        harness={harness}
                        className="size-3.5 shrink-0"
                      />
                      <span>{HARNESS_LABEL[harness].toUpperCase()}</span>
                    </div>
                    <Shimmer duration={1.6}>
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
                      `${HARNESS_TITLE[harness]} could not answer this side question.`}
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

          <div
            ref={btwComposerRef}
            className="shrink-0 border-t border-content/10 px-3.5 py-3"
          >
            <Composer
              key={openThreadId}
              compact
              enabled={!running}
              disabled={running}
              focused={!running}
              harness={harness}
              model={selectedModel}
              modelSettings={selectedModelSettings}
              runtimeMode={DEFAULT_RUNTIME_MODE}
              cwd={cwd ?? "~"}
              executionCwd={cwd ?? "~"}
              sessionId={openThreadId}
              hideProjectPicker
              hideBranchPicker
              hideTopBar
              placeholder="Ask a side question…"
              inputAriaLabel="By-the-way question"
              allowedModelHarnesses={[harness]}
              initialDraft={draftText}
              draftResetToken={draftResetToken}
              onFocus={() => {}}
              onCwdChange={() => {}}
              onModelChange={handleModelChange}
              onModelSettingsChange={handleSettingsChange}
              onRuntimeModeChange={() => {}}
              onSubmit={(text) => {
                if (running) return false;
                submit(text);
                return true;
              }}
              onDraftChange={setDraftText}
            />
            {running ? (
              <div
                className="mt-2 flex items-center gap-1.5 text-[11px] text-content/40"
                role="status"
              >
                <span
                  className="size-1.5 rounded-full bg-amber-300/80"
                  aria-hidden
                />
                {HARNESS_TITLE[harness]} is answering separately; the main turn
                is unchanged.
              </div>
            ) : null}
          </div>
        </Popover>
      ) : null}
    </span>
  );
}
