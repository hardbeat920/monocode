import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { AgentTranscript } from "../../src/features/sessions/ui/AgentTranscript";
import { MonoComposer } from "../../src/features/monos/ui/MonoComposer";
import { MessageQueue } from "../../src/features/sessions/ui/MessageQueue";
import { visibleMonoCompletionMessages } from "../../src/features/monos/model/monoMessaging";
import {
  newSession,
  type Block,
  type QueuedMessage,
} from "../../src/features/sessions/model/session";
import {
  saveTranscriptAnchor,
  saveTranscriptLayout,
} from "../../src/features/settings/model/appearance";
import "../../src/styles/index.css";

saveTranscriptAnchor(false);
saveTranscriptLayout("chat");

// Settled plain text isolates layout during scrolling from streaming, image
// loading and asynchronous syntax highlighting. Turns deliberately vary in size.
const blocks: Block[] = Array.from({ length: 20 }, (_, index): Block[] => [
  {
    id: `user-${index}`,
    role: "user",
    text: `Prompt ${index}: explain this behavior and preserve my place while reading.`,
  },
  {
    id: `reply-${index}`,
    role: "assistant",
    text: Array.from(
      { length: 8 + (index % 5) * 8 },
      (_, paragraph) =>
        `Paragraph ${paragraph}. This is a settled answer with enough text to exercise variable message heights.`,
    ).join("\n\n"),
  },
]).flat();

// Like the transcript pool, parking detaches the transcript from the page.
const root = document.getElementById("root")!;
const host = document.createElement("div");
host.className = "contents";
root.appendChild(host);

function Fixture() {
  const [parked, setParked] = useState(false);
  useEffect(() => {
    Object.assign(window, {
      parkTranscript: () => {
        host.remove();
        setParked(true);
      },
      showTranscript: () => {
        root.appendChild(host);
        setParked(false);
      },
    });
  }, []);
  return (
    <AgentTranscript
      blocks={blocks}
      initialTurns={20}
      visible={!parked}
      parked={parked}
    />
  );
}

const monoAnswer =
  "A reply arriving in chunks that should glide smoothly as the conversation grows. ".repeat(
    35,
  );

function MonoFixture() {
  const history = blocks.slice(-12);
  const prompt: Block = {
    id: "mono-prompt",
    role: "user",
    text: "Explain this in detail",
  };
  const [busy, setBusy] = useState(true);
  const [conversation, setConversation] = useState<Block[]>([
    ...history,
    prompt,
    { id: "mono-answer", role: "assistant", text: monoAnswer, streaming: true },
  ]);
  useEffect(() => {
    Object.assign(window, {
      completeMonoReply: (text = monoAnswer) => {
        setConversation([
          ...history,
          prompt,
          { id: "mono-answer", role: "assistant", text, streaming: false },
        ]);
        setBusy(false);
      },
    });
  }, []);
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        <AgentTranscript
          blocks={conversation}
          busy={busy}
          agentName="Mono"
          agentMascot={{ mascot: "crab", color: "#aaf" }}
          bottomAligned
          inlineWork
        />
      </div>
      <MonoComposer
        sessionId="browser-mono"
        name="Mono"
        enabled={false}
        onSubmit={() => false}
        controls={<span className="h-6.5">Model · Effort · Context</span>}
      />
    </div>
  );
}

function MonoCompletionFixture() {
  const history = new URLSearchParams(location.search).has("short")
    ? []
    : blocks.slice(-12);
  const start = Date.now();
  const launched = {
    sessionId: "app-mono-review",
    cwd: "/repo",
    title: "Review the changes",
    harness: "codex" as const,
    model: "gpt-6",
  };
  const original: Block[] = [
    ...history,
    {
      id: "mono-request",
      role: "user",
      text: "Start a review session and report back",
      startedAt: start,
      durationMs: 1000,
      monoSpawnedSessions: [launched],
    },
    {
      id: "mono-launch",
      role: "tool",
      text: "Start a session",
      tool: { kind: "execute", status: "completed" },
    },
    {
      id: "mono-handoff",
      role: "assistant",
      text: "I started the review session. I’ll report back when it finishes.",
    },
  ];
  const completion: Block = {
    id: "mono-completion",
    role: "user",
    text: "Review complete",
    internal: true,
    appRequestId: "mono-completion-batch-review",
    startedAt: start + 5000,
  };
  const [conversation, setConversation] = useState(original);
  const [notifications, setNotifications] = useState<QueuedMessage[]>([]);
  const [session] = useState(() => newSession("codex", "/repo"));
  const [busy, setBusy] = useState(false);
  const [state, setState] = useState<"working" | "done">("working");
  useEffect(() => {
    Object.assign(window, {
      queueMonoCompletion: () => {
        setNotifications([
          {
            id: completion.id,
            text: completion.text,
            attachments: [],
            monoSessionCompletion: {
              sessionId: launched.sessionId,
              title: launched.title,
              status: "completed",
            },
          },
        ]);
        setState("done");
      },
      beginMonoCompletion: () => {
        setNotifications([]);
        setConversation([...original, completion]);
        setBusy(true);
        setState("done");
      },
      finishMonoCompletion: () => {
        setConversation([
          ...original,
          { ...completion, durationMs: 1000 },
          {
            id: "mono-report",
            role: "assistant",
            text: "The review is complete. Here are the results. ".repeat(40),
          },
        ]);
        setBusy(false);
      },
    });
  }, []);
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1">
        <AgentTranscript
          blocks={conversation}
          busy={busy}
          agentName="Mono"
          agentMascot={{ mascot: "crab", color: "#aaf" }}
          bottomAligned
          inlineWork
          onOpenSession={() => {}}
          onShowWork={() => {}}
          launchedSessions={
            new Map([[launched.sessionId, { state, title: launched.title }]])
          }
        />
      </div>
      <MessageQueue
        messages={visibleMonoCompletionMessages({
          ...session,
          blocks: conversation,
          busy,
          queuedMessages: notifications,
          queueStatus: "active",
        })}
        variant="messages"
        onResume={() => {}}
      />
      <MonoComposer
        sessionId="completion-mono"
        name="Mono"
        enabled={false}
        onSubmit={() => false}
        controls={<span className="h-6.5">Model · Effort · Context</span>}
      />
    </div>
  );
}

const params = new URLSearchParams(location.search);
const mono = params.has("mono");
const completion = params.has("monoCompletion");
if (mono || completion) root.style.width = "540px";
createRoot(host).render(
  completion ? <MonoCompletionFixture /> : mono ? <MonoFixture /> : <Fixture />,
);
