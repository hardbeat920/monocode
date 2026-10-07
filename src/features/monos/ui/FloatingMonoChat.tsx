import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ExternalLink, Square, X } from "../../../shared/ui/icons";
import { PixelMascot } from "../../projects/ui/PixelMascot";
import { AgentTranscript } from "../../sessions/ui/AgentTranscript";
import { QuestionForm } from "../../sessions/ui/QuestionForm";
import { revokeAttachment } from "../../sessions/model/attachments";
import { useMonoTranscript } from "../hooks/useMonoTranscript";
import { monoState } from "../model/mono";
import {
  monoMessageDeliveries,
  monoPendingTranscriptBlocks,
} from "../model/monoMessaging";
import { sessionWorkCwd, type Session } from "../../sessions/model/session";
import {
  FLOATING_MONO_CHANGED,
  floatingMonoAttachments,
  type FloatingMonoAction,
  type FloatingMonoEntry,
  type FloatingMonoView,
} from "../model/floatingMono";
import { MonoComposer } from "./MonoComposer";
import { MonoStatus } from "./MonoStatus";
import { MONO_PAGE_TURNS } from "../../sessions/data/sessionStore";

const EMPTY: FloatingMonoView = {
  monos: [],
  monoId: null,
  session: null,
  error: null,
};
const BUTTON =
  "grid size-7 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/8 hover:text-content disabled:opacity-30";
const SURFACE =
  "body-glass flex h-full min-h-0 flex-col overflow-hidden rounded-2xl font-sans text-content";

export function FloatingMonoChat({ onShown }: { onShown: () => void }) {
  const [view, setView] = useState(EMPTY);
  const [error, setError] = useState<string | null>(null);
  const [focus, setFocus] = useState(0);
  const selectedId = useRef(view.monoId);
  selectedId.current = view.monoId;
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    let received = false;
    let selected: string | null = null;
    const receive = (next: FloatingMonoView) => {
      if (disposed) return;
      if (next.monoId !== selected) {
        selected = next.monoId;
        setError(null);
        setFocus((n) => n + 1);
      }
      setView(next);
      onShown();
    };
    void getCurrentWebviewWindow()
      .listen<FloatingMonoView>(FLOATING_MONO_CHANGED, (event) => {
        received = true;
        receive(event.payload);
      })
      .then(async (unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        stop = unlisten;
        const initial = await invoke<FloatingMonoView>("mono_chat_state");
        if (!received) receive(initial);
      })
      .catch((reason) => {
        if (!disposed) setError(String(reason));
      });
    const onFocus = () => {
      onShown();
      setFocus((n) => n + 1);
    };
    window.addEventListener("focus", onFocus);
    return () => {
      disposed = true;
      stop?.();
      window.removeEventListener("focus", onFocus);
    };
  }, [onShown]);

  useEffect(() => {
    // Present only after the loading screen has mounted. A hidden WKWebView
    // can suspend animation frames, so this must not wait on rAF to open.
    let disposed = false;
    void invoke("mono_chat_ready").catch((reason) => {
      if (!disposed) setError(String(reason));
    });
    return () => {
      disposed = true;
    };
  }, []);

  const mono = view.monos.find((entry) => entry.id === view.monoId);
  const action = useCallback(
    async (next: FloatingMonoAction): Promise<boolean> => {
      if (!view.monoId) return false;
      const id = view.monoId;
      setError(null);
      try {
        await invoke("mono_chat_action", { monoId: id, action: next });
        return true;
      } catch (reason) {
        if (selectedId.current === id)
          setError(reason instanceof Error ? reason.message : String(reason));
        return false;
      }
    },
    [view.monoId],
  );
  const state = view.session
    ? monoState(view.session)
    : { status: "idle" as const };

  if (!mono || !view.session) {
    const failure = error ?? view.error;
    return (
      <div
        data-floating-mono
        data-floating-mono-loading
        aria-busy={!failure}
        className={`${SURFACE} relative`}
        style={{ backgroundColor: "var(--color-background-base)" }}
      >
        <header
          data-tauri-drag-region
          className="absolute inset-x-0 top-0 flex justify-end px-3 py-3"
        >
          <button
            type="button"
            aria-label="Hide chat"
            title="Hide chat"
            className={BUTTON}
            onClick={() => void getCurrentWindow().hide()}
          >
            <X className="size-4" />
          </button>
        </header>
        <div
          role={failure ? "alert" : "status"}
          className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 px-8 text-center"
        >
          <img src="/monocode.png" alt="" className="size-18 object-contain" />
          <p className="text-[13px] text-content/50">
            {failure ?? "Loading conversation…"}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div data-floating-mono className={SURFACE}>
      <header
        data-tauri-drag-region
        className="flex shrink-0 items-center gap-2 border-b border-content/8 px-3 py-3"
      >
        {mono ? (
          <PixelMascot
            name={mono.mascot}
            color={mono.color}
            status={state.status}
            className="pointer-events-none size-7 shrink-0"
          />
        ) : null}
        <div data-tauri-drag-region className="flex min-w-0 flex-1 flex-col">
          <h1
            data-tauri-drag-region
            className="truncate text-[13px] leading-4 font-semibold"
          >
            {mono?.name ?? "Mono"}
          </h1>
          {mono ? (
            <MonoStatus
              state={state}
              color={mono.color}
              className="pointer-events-none text-[11px] leading-3.5 text-content/45"
            />
          ) : null}
        </div>
        {view.session?.busy ? (
          <button
            type="button"
            aria-label="Stop reply"
            title="Stop reply (Escape)"
            className={BUTTON}
            onClick={() => void action({ kind: "stop" })}
          >
            <Square className="size-3 fill-current" />
          </button>
        ) : null}
        <button
          type="button"
          aria-label="Open in MonoCode"
          title="Open in MonoCode"
          disabled={!view.session}
          className={BUTTON}
          onClick={() => void action({ kind: "reveal" })}
        >
          <ExternalLink className="size-3.5" />
        </button>
        <button
          type="button"
          aria-label="Hide chat"
          title="Hide chat"
          className={BUTTON}
          onClick={() => void getCurrentWindow().hide()}
        >
          <X className="size-4" />
        </button>
      </header>
      {error || view.error ? (
        <p
          role="alert"
          className="shrink-0 border-b border-content/8 px-3 py-2 text-[12px] text-red-400"
        >
          {error ?? view.error}
        </p>
      ) : null}
      <FloatingConversation
        key={view.session.id}
        mono={mono}
        session={view.session}
        focus={focus}
        action={action}
      />
    </div>
  );
}

function FloatingConversation({
  mono,
  session,
  focus,
  action,
}: {
  mono: FloatingMonoEntry;
  session: Session;
  focus: number;
  action: (action: FloatingMonoAction) => Promise<boolean>;
}) {
  const transcript = useMonoTranscript(session, true);
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      onKeyDown={(event) => {
        if (
          event.key === "Escape" &&
          !event.nativeEvent.isComposing &&
          session.busy
        ) {
          event.preventDefault();
          void action({ kind: "stop" });
        }
      }}
    >
      <div className="@container relative min-h-0 flex-1">
        {!session.blocks.length ? (
          <div className="pointer-events-none absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 px-8 text-center">
            <PixelMascot
              name={mono.mascot}
              color={mono.color}
              className="size-12"
            />
            <p className="text-[13px] leading-6 text-content/45">
              Say hello to {mono.name}, ask a question, or hand over some work.
            </p>
          </div>
        ) : null}
        <AgentTranscript
          blocks={
            transcript.viewingOlderPage
              ? transcript.blocks
              : monoPendingTranscriptBlocks(session, transcript.blocks)
          }
          historicalBlockIds={transcript.historicalBlockIds}
          initialTurns={MONO_PAGE_TURNS}
          pageSize={MONO_PAGE_TURNS}
          busy={!!session.busy && !transcript.viewingOlderPage}
          cwd={sessionWorkCwd(session)}
          agentName={mono.name}
          agentMascot={mono}
          bottomAligned
          inlineWork
          daySeparators
          hideTurnMetrics
          visible
          messageDeliveries={monoMessageDeliveries(session)}
          hasEarlier={transcript.hasEarlier}
          loadEarlierOnScroll
          onLoadEarlier={transcript.loadEarlier}
          onReturnToLatest={
            transcript.viewingOlderPage ? transcript.latest : undefined
          }
          harness={session.harness}
          model={session.model}
          modelSettings={session.modelSettings}
          pendingQuestion={!!session.pendingQuestion}
          backgroundTasks={session.backgroundTasks}
          onApproval={(requestId, decision) =>
            void action({ kind: "approval", requestId, decision })
          }
          onOpenFile={(path) => void action({ kind: "openFile", path })}
          onOpenArtifact={(id) => void action({ kind: "openArtifact", id })}
          onOpenDiff={() => void action({ kind: "reveal" })}
          onShowWork={() => void action({ kind: "reveal" })}
          onShowSessions={() => void action({ kind: "reveal" })}
        />
      </div>
      {session.pendingQuestion ? (
        <div className="max-h-[45%] shrink-0 overflow-y-auto px-2 pb-2">
          <QuestionForm
            prompt={session.pendingQuestion}
            onReply={(requestId, reply) =>
              void action({ kind: "question", requestId, reply })
            }
            onInteraction={(requestId) =>
              void action({ kind: "questionInteraction", requestId })
            }
          />
        </div>
      ) : null}
      {session.usageLimit ? (
        <div className="shrink-0 px-3 pb-2 text-[12px] text-content/60">
          <p>The provider’s usage limit was reached.</p>
          <button
            type="button"
            className="mt-1 text-accent hover:underline"
            onClick={() => void action({ kind: "resume" })}
          >
            Try again
          </button>
          <span className="mx-2 text-content/25">·</span>
          <button
            type="button"
            className="text-accent hover:underline"
            onClick={() => void action({ kind: "reveal" })}
          >
            Change model in MonoCode
          </button>
        </div>
      ) : null}
      <div className="shrink-0 px-1 pb-1">
        <MonoComposer
          sessionId={`floating:${session.id}`}
          name={mono.name}
          enabled={!session.worktreeRemoved}
          focusToken={focus}
          onSubmit={async (text, attachments) => {
            const accepted = await action({
              kind: "submit",
              text,
              attachments: floatingMonoAttachments(attachments),
            });
            // Only bytes/paths cross into the owner; its renderer never owns
            // these composer preview URLs.
            if (accepted) attachments.forEach(revokeAttachment);
            return accepted;
          }}
        />
      </div>
    </div>
  );
}
