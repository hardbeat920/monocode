// Browser smoke fixture. IPC is mocked; it never starts a real agent or model turn.
import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
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
    if (cmd.startsWith("harness_resolve_")) return { path: "/test/agent" };
    if (
      cmd === "list_project_files" ||
      cmd.includes("list_") ||
      cmd.includes("read_dir")
    )
      return [];
    if (cmd === "pty_spawn") {
      setTimeout(() => {
        void emit("pty-data", {
          id: args?.id,
          data: btoa("\x1b[?2004hAgent ready. Type / for commands.\r\n> "),
        });
      }, 100);
      return;
    }
    if (cmd === "pty_status") return { foreground: "agent" };
    return null;
  },
  { shouldMockEvents: true },
);
const { Composer } = await import("../src/chrome/Composer");
const { AgentCliView } = await import("../src/surfaces/AgentCliView");
const { newSession } = await import("../src/lib/session");
function Fixture() {
  const [harness, setHarness] = useState<"codex" | "claude">("codex");
  const [native, setNative] = useState<{ command?: string } | null>(null);
  const [session] = useState(() =>
    newSession("codex", "/repo", "codex:gpt-6-astra"),
  );
  return (
    <main className="flex h-screen flex-col bg-background-base text-content">
      <nav className="p-3">
        <button
          onClick={() => setHarness(harness === "codex" ? "claude" : "codex")}
        >
          Switch provider
        </button>{" "}
        · {harness}
      </nav>
      {native ? (
        <AgentCliView
          session={{ ...session, harness }}
          initialCommand={native.command}
          active
          onClose={() => setNative(null)}
        />
      ) : null}
      <div
        className={native ? "hidden" : "mx-auto mt-auto mb-12 w-full max-w-4xl"}
      >
        <Composer
          harness={harness}
          model={`${harness}:test`}
          runtimeMode="supervised"
          cwd="/repo"
          executionCwd="/repo"
          focused={!native}
          hotkeys={!native}
          enabled={!native}
          hideBranchPicker
          onFocus={() => {}}
          onCwdChange={() => {}}
          onModelChange={() => {}}
          onRuntimeModeChange={() => {}}
          onSubmit={(text) => calls.push({ modelTurn: text })}
          onAgentCommand={(command) => {
            calls.push({ agentCommand: command ?? "" });
            setNative({ command });
            return true;
          }}
        />
      </div>
    </main>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
