// Browser regression fixture using the real SessionPane. IPC is mocked;
// commands cannot start a real agent, PTY or model turn here.
import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import "../src/index.css";
const calls: unknown[] = [];
Object.assign(window, { testCalls: calls });
mockWindows("main");
mockIPC(
  (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_skills")
      return [
        {
          name: "research",
          description: "Research skill",
          path: "/repo/.agents/skills/research/SKILL.md",
          scope: "project",
          source: "agents",
        },
      ];
    if (cmd === "home_dir" || cmd === "default_cwd") return "/repo";
    if (cmd === "git_status")
      return { branch: "main", changes: [], isRepo: true };
    if (cmd.includes("list_") || cmd.includes("read_dir")) return [];
    if (cmd.startsWith("harness_resolve_")) return { path: "/test/agent" };
    return null;
  },
  { shouldMockEvents: true },
);
const { SessionPane } = await import("../src/surfaces/SessionPane");
const { newSession } = await import("../src/lib/session");
const { setHarnessModels } = await import("../src/lib/models");
for (const harness of ["codex", "claude"] as const)
  setHarnessModels(harness, [
    {
      id: `${harness}:current`,
      harness,
      name: `${harness} current`,
      nativeId: "current",
      settings: [
        {
          id: "effort",
          label: "Effort",
          kind: "select",
          value: "low",
          options: [
            { value: "low", label: "Low" },
            { value: "high", label: "High" },
          ],
        },
        {
          id: "fast",
          label: "Fast",
          kind: "toggle",
          value: "false",
          options: [],
        },
      ],
    },
    {
      id: `${harness}:next`,
      harness,
      name: `${harness} next`,
      nativeId: "next",
    },
  ]);
function Fixture() {
  const [session, setSession] = useState(() => ({
    ...newSession("codex", "/repo", "codex:current"),
    id: "existing-chat",
    providerSessionId: "existing-provider-session",
    context: { used: 32000, window: 128000 },
    blocks: [
      {
        id: "user",
        role: "user" as const,
        text: "Keep this conversation in the chat UI.",
      },
      {
        id: "answer",
        role: "assistant" as const,
        text: "Existing conversation remains visible.",
      },
    ],
  }));
  Object.assign(window, { testSession: session });
  return (
    <main className="flex h-screen flex-col bg-background-base text-content">
      <nav className="flex shrink-0 gap-4 p-3 text-sm">
        <button
          onClick={() =>
            setSession((current) => {
              const harness = current.harness === "codex" ? "claude" : "codex";
              return { ...current, harness, model: `${harness}:current` };
            })
          }
        >
          Switch provider
        </button>
        <button
          onClick={() =>
            setSession((current) => ({ ...current, busy: !current.busy }))
          }
        >
          Toggle busy
        </button>
        <button
          onClick={() => setSession((current) => ({ ...current, blocks: [] }))}
        >
          Empty chat
        </button>
        <span>{session.harness}</span>
      </nav>
      <SessionPane
        session={session}
        visible
        focused
        inSplit={false}
        composerFocused
        recents={[]}
        hideProjectPicker
        onFocus={() => {}}
        onClose={() => {}}
        onCwdChange={() => {}}
        onBranchChange={() => {}}
        onModelChange={(id, harness, model) => {
          calls.push({ modelChange: { id, harness, model } });
          setSession((current) => ({ ...current, harness, model }));
        }}
        onModelSettingsChange={(id, modelSettings) => {
          calls.push({ modelSettings: { id, modelSettings } });
          setSession((current) => ({ ...current, modelSettings }));
        }}
        onRuntimeModeChange={(id, runtimeMode) => {
          calls.push({ permissionChange: { id, runtimeMode } });
          setSession((current) => ({ ...current, runtimeMode }));
        }}
        onSubmit={(id, text, attachments, options) =>
          calls.push({ modelTurn: { id, text, attachments, options } })
        }
        onStop={(id) => {
          calls.push({ stop: id });
          setSession((current) => ({ ...current, busy: false }));
        }}
        onCompactContext={(id) => {
          calls.push({ compact: id });
          return true;
        }}
        onDeleteQueuedMessage={() => {}}
        onEditQueuedMessage={() => {}}
        onQueuedMessageEditingChange={() => {}}
        onSteerQueuedMessage={() => {}}
        onResumeQueue={() => {}}
        onApproval={() => {}}
        onQuestionReply={() => {}}
        onOpenFile={() => {}}
        onOpenDiff={(path, context) => calls.push({ diff: { path, context } })}
        onOpenPlan={() => {}}
        onBuildPlan={() => {}}
        onNewTerminal={() => calls.push({ newTerminal: true })}
      />
    </main>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
